use crate::io::reader::{CsvRow, TransactionReader};
use crate::io::writer::AccountWriter;
use crate::model::account::{Account, AccountError};
use crate::model::transaction::{AccountTransactionInput, TransactionParseError};
use crate::model::types::{ClientId, NotSend};
use crate::processor::TransactionProcessorError::{
    ClientAccountError, CsvReaderError, CsvWriterError, LineParseError,
};
use csv::Error as CsvError;
use std::collections::HashMap;
use std::io::{Error as IoError, stdout};
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TransactionProcessorError {
    #[error("CSV line parsing error: {0}")]
    LineParseError(TransactionParseError),
    #[error("Account error: {0}")]
    ClientAccountError(AccountError),
    #[error("CSV reader error: {0}")]
    CsvReaderError(CsvError),
    #[error("CSV writer error: {0}")]
    CsvWriterError(IoError),
}

pub struct TransactionProcessor {
    accounts: HashMap<ClientId, Account>,
    fail_file_on_error: bool,
    _ns: NotSend,
}

impl TransactionProcessor {
    pub fn new(fail_file_on_error: bool) -> Self {
        Self {
            accounts: HashMap::new(),
            fail_file_on_error,
            _ns: NotSend::new(),
        }
    }

    pub fn process_file(&mut self, file_path: &str) -> Result<(), TransactionProcessorError> {
        let path = Path::new(file_path);
        let mut reader = TransactionReader::from_path(path).map_err(|e| CsvReaderError(e))?;

        for result in reader.records() {
            let csv_row = match result {
                Ok(row) => row,
                Err(err) => {
                    tracing::warn!("Skipping malformed row: {err}");
                    if self.fail_file_on_error {
                        return Err(CsvReaderError(err));
                    }
                    continue;
                }
            };

            let id = csv_row.transaction_id;
            match self.process_transaction_row(csv_row) {
                Ok(_) => tracing::debug!("Transaction processed successfully: {id}"),
                Err(err) => {
                    tracing::warn!("Transaction {id} error: {err}");
                    if self.fail_file_on_error {
                        return Err(err);
                    }
                }
            }
        }

        self.output_accounts_csv()
            .map_err(|err| CsvWriterError(err))
    }

    fn process_transaction_row(
        &mut self,
        next_line: CsvRow,
    ) -> Result<(), TransactionProcessorError> {
        let account_input = match AccountTransactionInput::create(next_line) {
            Ok(input) => input,
            Err(error) => return Err(LineParseError(error)),
        };

        let client_account = self
            .accounts
            .entry(account_input.client_id)
            .or_insert(Account::new(account_input.client_id));

        match client_account.handle_transaction(&account_input) {
            Ok(_) => Ok(()),
            Err(error) => {
                tracing::warn!("Unable to process transaction due to: {error}");
                Err(ClientAccountError(error))
            }
        }
    }

    fn output_accounts_csv(&self) -> Result<(), IoError> {
        tracing::info!("Printing {} accounts", self.accounts.len());
        let mut writer = AccountWriter::new(stdout().lock());
        writer.write_all(&self.accounts)
    }

    #[cfg(test)]
    fn account_count(&self) -> usize {
        self.accounts.len()
    }

    #[cfg(test)]
    fn get_account(&self, client_id: &ClientId) -> Option<&Account> {
        self.accounts.get(client_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_csv(name: &str, content: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path
    }

    // --- Constructor ---

    #[test]
    fn new_processor_has_no_accounts() {
        let proc = TransactionProcessor::new(false);
        assert_eq!(proc.account_count(), 0);
    }

    #[test]
    fn new_processor_stores_fail_flag() {
        let proc = TransactionProcessor::new(true);
        assert!(proc.fail_file_on_error);
    }

    // --- process_file: happy path ---

    #[test]
    fn process_single_deposit() {
        let csv = "type, client, tx, amount\ndeposit, 1, 1, 10.0\n";
        let path = write_csv("proc_single_deposit.csv", csv);
        let mut proc = TransactionProcessor::new(false);
        proc.process_file(path.to_str().unwrap()).unwrap();
        assert_eq!(proc.account_count(), 1);
        let acct = proc.get_account(&ClientId(1)).unwrap();
        assert_eq!(acct.to_csv_row(), "1,10.0000,0.0000,10.0000,false");
    }

    #[test]
    fn process_multiple_clients() {
        let csv = "type, client, tx, amount\ndeposit, 1, 1, 10.0\ndeposit, 2, 2, 20.0\n";
        let path = write_csv("proc_multi_client.csv", csv);
        let mut proc = TransactionProcessor::new(false);
        proc.process_file(path.to_str().unwrap()).unwrap();
        assert_eq!(proc.account_count(), 2);
    }

    #[test]
    fn process_deposit_and_withdrawal() {
        let csv = "type, client, tx, amount\ndeposit, 1, 1, 10.0\nwithdrawal, 1, 2, 3.0\n";
        let path = write_csv("proc_dep_wd.csv", csv);
        let mut proc = TransactionProcessor::new(false);
        proc.process_file(path.to_str().unwrap()).unwrap();
        let acct = proc.get_account(&ClientId(1)).unwrap();
        assert_eq!(acct.to_csv_row(), "1,7.0000,0.0000,7.0000,false");
    }

    #[test]
    fn process_dispute_resolve_flow() {
        let csv = "type, client, tx, amount\ndeposit, 1, 1, 10.0\ndispute, 1, 1,\nresolve, 1, 1,\n";
        let path = write_csv("proc_dispute_resolve.csv", csv);
        let mut proc = TransactionProcessor::new(false);
        proc.process_file(path.to_str().unwrap()).unwrap();
        let acct = proc.get_account(&ClientId(1)).unwrap();
        assert_eq!(acct.to_csv_row(), "1,10.0000,0.0000,10.0000,false");
    }

    #[test]
    fn process_dispute_chargeback_freezes() {
        let csv =
            "type, client, tx, amount\ndeposit, 1, 1, 10.0\ndispute, 1, 1,\nchargeback, 1, 1,\n";
        let path = write_csv("proc_chargeback.csv", csv);
        let mut proc = TransactionProcessor::new(false);
        proc.process_file(path.to_str().unwrap()).unwrap();
        let acct = proc.get_account(&ClientId(1)).unwrap();
        assert_eq!(acct.to_csv_row(), "1,0.0000,0.0000,0.0000,true");
    }

    // --- Error handling: skip mode ---

    #[test]
    fn skip_mode_continues_on_bad_row() {
        let csv = "type, client, tx, amount\ndeposit, 1, 1, 10.0\ngarbage\ndeposit, 2, 2, 5.0\n";
        let path = write_csv("proc_skip_bad.csv", csv);
        let mut proc = TransactionProcessor::new(false);
        proc.process_file(path.to_str().unwrap()).unwrap();
        assert_eq!(proc.account_count(), 2);
    }

    #[test]
    fn skip_mode_continues_on_insufficient_funds() {
        let csv = "type, client, tx, amount\ndeposit, 1, 1, 5.0\nwithdrawal, 1, 2, 100.0\ndeposit, 1, 3, 20.0\n";
        let path = write_csv("proc_skip_insuf.csv", csv);
        let mut proc = TransactionProcessor::new(false);
        proc.process_file(path.to_str().unwrap()).unwrap();
        let acct = proc.get_account(&ClientId(1)).unwrap();
        assert_eq!(acct.to_csv_row(), "1,25.0000,0.0000,25.0000,false");
    }

    #[test]
    fn skip_mode_continues_on_unknown_type() {
        let csv =
            "type, client, tx, amount\ndeposit, 1, 1, 10.0\nfoo, 1, 2, 5.0\ndeposit, 1, 3, 5.0\n";
        let path = write_csv("proc_skip_unknown.csv", csv);
        let mut proc = TransactionProcessor::new(false);
        proc.process_file(path.to_str().unwrap()).unwrap();
        let acct = proc.get_account(&ClientId(1)).unwrap();
        assert_eq!(acct.to_csv_row(), "1,15.0000,0.0000,15.0000,false");
    }

    // --- Error handling: fail mode ---

    #[test]
    fn fail_mode_aborts_on_transaction_error() {
        let csv = "type, client, tx, amount\ndeposit, 1, 1, 5.0\nwithdrawal, 1, 2, 100.0\ndeposit, 1, 3, 20.0\n";
        let path = write_csv("proc_fail_abort.csv", csv);
        let mut proc = TransactionProcessor::new(true);
        let result = proc.process_file(path.to_str().unwrap());
        assert!(result.is_err());
        // Only first deposit processed, third never reached
        let acct = proc.get_account(&ClientId(1)).unwrap();
        assert_eq!(acct.to_csv_row(), "1,5.0000,0.0000,5.0000,false");
    }

    #[test]
    fn fail_mode_aborts_on_csv_reader_error() {
        let csv = "type, client, tx, amount\ndeposit, 1, 1, 10.0\ndeposit, notanumber, 2, 5.0\ndeposit, 1, 3, 20.0\n";
        let path = write_csv("proc_fail_csv_reader.csv", csv);
        let mut proc = TransactionProcessor::new(true);
        let result = proc.process_file(path.to_str().unwrap());
        assert!(matches!(result, Err(CsvReaderError(_))));
        // Only first deposit processed; malformed row aborted file
        assert_eq!(proc.account_count(), 1);
    }

    #[test]
    fn fail_mode_aborts_on_parse_error() {
        let csv =
            "type, client, tx, amount\ndeposit, 1, 1, 10.0\nfoo, 1, 2, 5.0\ndeposit, 1, 3, 5.0\n";
        let path = write_csv("proc_fail_parse.csv", csv);
        let mut proc = TransactionProcessor::new(true);
        let result = proc.process_file(path.to_str().unwrap());
        assert!(result.is_err());
        assert_eq!(proc.account_count(), 1);
    }

    // --- File errors ---

    #[test]
    fn nonexistent_file_returns_error() {
        let mut proc = TransactionProcessor::new(false);
        let result = proc.process_file("nonexistent_file_12345.csv");
        assert!(matches!(result, Err(CsvReaderError(_))));
    }

    // --- Cross-client isolation ---

    #[test]
    fn clients_are_isolated() {
        let csv = "type, client, tx, amount\ndeposit, 1, 1, 10.0\ndeposit, 2, 2, 20.0\nwithdrawal, 1, 3, 5.0\n";
        let path = write_csv("proc_isolation.csv", csv);
        let mut proc = TransactionProcessor::new(false);
        proc.process_file(path.to_str().unwrap()).unwrap();
        let a1 = proc.get_account(&ClientId(1)).unwrap();
        let a2 = proc.get_account(&ClientId(2)).unwrap();
        assert_eq!(a1.to_csv_row(), "1,5.0000,0.0000,5.0000,false");
        assert_eq!(a2.to_csv_row(), "2,20.0000,0.0000,20.0000,false");
    }

    #[test]
    fn frozen_client_does_not_affect_other() {
        let csv = "type, client, tx, amount\ndeposit, 1, 1, 10.0\ndeposit, 2, 2, 20.0\ndispute, 1, 1,\nchargeback, 1, 1,\ndeposit, 2, 3, 5.0\n";
        let path = write_csv("proc_frozen_iso.csv", csv);
        let mut proc = TransactionProcessor::new(false);
        proc.process_file(path.to_str().unwrap()).unwrap();
        let a1 = proc.get_account(&ClientId(1)).unwrap();
        let a2 = proc.get_account(&ClientId(2)).unwrap();
        assert_eq!(a1.to_csv_row(), "1,0.0000,0.0000,0.0000,true");
        assert_eq!(a2.to_csv_row(), "2,25.0000,0.0000,25.0000,false");
    }

    // --- Empty file ---

    #[test]
    fn empty_file_produces_no_accounts() {
        let csv = "type, client, tx, amount\n";
        let path = write_csv("proc_empty.csv", csv);
        let mut proc = TransactionProcessor::new(false);
        proc.process_file(path.to_str().unwrap()).unwrap();
        assert_eq!(proc.account_count(), 0);
    }

    // --- Decimal precision ---

    #[test]
    fn four_decimal_precision_preserved() {
        let csv = "type, client, tx, amount\ndeposit, 1, 1, 1.2345\n";
        let path = write_csv("proc_precision.csv", csv);
        let mut proc = TransactionProcessor::new(false);
        proc.process_file(path.to_str().unwrap()).unwrap();
        let acct = proc.get_account(&ClientId(1)).unwrap();
        assert_eq!(acct.to_csv_row(), "1,1.2345,0.0000,1.2345,false");
    }
}
