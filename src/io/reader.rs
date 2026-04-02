use std::fs::File;
use csv::{Error as CsvError, Reader, ReaderBuilder, Trim};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct CsvRow {
    #[serde(rename = "type")]
    pub transaction_type: String,
    #[serde(rename = "client")]
    pub client_id: u16,
    #[serde(rename = "tx")]
    pub transaction_id: u32,
    pub amount: Option<f64>,
}

pub struct TransactionReader {
    reader: Reader<File>,
}

impl TransactionReader {
    pub fn from_path(path: &Path) -> Result<Self, CsvError> {
        let reader = ReaderBuilder::new()
            .trim(Trim::All)
            .flexible(true)
            .from_path(path)?;
        Ok(Self { reader })
    }

    pub fn records(&mut self) -> impl Iterator<Item = Result<CsvRow, CsvError>> {
        self.reader.deserialize::<CsvRow>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_csv(name: &str, content: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("reader_test_{name}.csv"));
        let mut f = File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path
    }

    #[test]
    fn reads_single_deposit() {
        let path = write_csv("single", "type, client, tx, amount\ndeposit, 1, 1, 1.0\n");
        let mut reader = TransactionReader::from_path(&path).unwrap();
        let rows: Vec<_> = reader.records().collect();
        assert_eq!(rows.len(), 1);
        let row = rows[0].as_ref().unwrap();
        assert_eq!(row.transaction_type, "deposit");
        assert_eq!(row.client_id, 1);
        assert_eq!(row.transaction_id, 1);
        assert!((row.amount.unwrap() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn reads_multiple_rows() {
        let csv = "type,client,tx,amount\ndeposit,1,1,10.0\nwithdrawal,1,2,5.0\n";
        let path = write_csv("multi", csv);
        let mut reader = TransactionReader::from_path(&path).unwrap();
        let rows: Vec<_> = reader.records().filter_map(|r| r.ok()).collect();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].transaction_type, "deposit");
        assert_eq!(rows[1].transaction_type, "withdrawal");
    }

    #[test]
    fn handles_whitespace_trimming() {
        let csv = "type , client , tx , amount\n  deposit , 1 , 1 , 2.5 \n";
        let path = write_csv("whitespace", csv);
        let mut reader = TransactionReader::from_path(&path).unwrap();
        let row = reader.records().next().unwrap().unwrap();
        assert_eq!(row.transaction_type, "deposit");
        assert_eq!(row.client_id, 1);
        assert!((row.amount.unwrap() - 2.5).abs() < f64::EPSILON);
    }

    #[test]
    fn handles_missing_amount() {
        let csv = "type,client,tx,amount\ndispute,1,1,\n";
        let path = write_csv("no_amount", csv);
        let mut reader = TransactionReader::from_path(&path).unwrap();
        let row = reader.records().next().unwrap().unwrap();
        assert_eq!(row.transaction_type, "dispute");
        assert!(row.amount.is_none());
    }

    #[test]
    fn flexible_allows_fewer_columns() {
        let csv = "type,client,tx,amount\ndispute,1,1\n";
        let path = write_csv("flexible", csv);
        let mut reader = TransactionReader::from_path(&path).unwrap();
        let row = reader.records().next().unwrap().unwrap();
        assert_eq!(row.transaction_type, "dispute");
        assert!(row.amount.is_none());
    }

    #[test]
    fn empty_file_returns_no_rows() {
        let path = write_csv("empty", "type,client,tx,amount\n");
        let mut reader = TransactionReader::from_path(&path).unwrap();
        let rows: Vec<_> = reader.records().collect();
        assert_eq!(rows.len(), 0);
    }

    #[test]
    fn nonexistent_file_returns_error() {
        let result = TransactionReader::from_path(Path::new("nonexistent_file_12345.csv"));
        assert!(result.is_err());
    }

    #[test]
    fn malformed_row_returns_error() {
        let csv = "type,client,tx,amount\ndeposit,notanumber,1,1.0\n";
        let path = write_csv("malformed", csv);
        let mut reader = TransactionReader::from_path(&path).unwrap();
        let result = reader.records().next().unwrap();
        assert!(result.is_err());
    }

    #[test]
    fn four_decimal_precision() {
        let csv = "type,client,tx,amount\ndeposit,1,1,1.2345\n";
        let path = write_csv("precision", csv);
        let mut reader = TransactionReader::from_path(&path).unwrap();
        let row = reader.records().next().unwrap().unwrap();
        assert!((row.amount.unwrap() - 1.2345).abs() < 1e-10);
    }

    #[test]
    fn all_transaction_types_parsed() {
        let csv = "type,client,tx,amount\ndeposit,1,1,1.0\nwithdrawal,1,2,1.0\ndispute,1,3,\nresolve,1,4,\nchargeback,1,5,\n";
        let path = write_csv("all_types", csv);
        let mut reader = TransactionReader::from_path(&path).unwrap();
        let rows: Vec<_> = reader.records().filter_map(|r| r.ok()).collect();
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0].transaction_type, "deposit");
        assert_eq!(rows[1].transaction_type, "withdrawal");
        assert_eq!(rows[2].transaction_type, "dispute");
        assert_eq!(rows[3].transaction_type, "resolve");
        assert_eq!(rows[4].transaction_type, "chargeback");
    }
}