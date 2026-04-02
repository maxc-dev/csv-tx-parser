use io::Error as IoError;
use crate::model::account::{Account, ACCOUNT_HEADER_ROW};
use crate::model::types::ClientId;
use std::collections::HashMap;
use std::io::{self, Write, BufWriter};

pub struct AccountWriter<W: Write> {
    writer: BufWriter<W>,
}

impl<W: Write> AccountWriter<W> {
    pub fn new(output: W) -> Self {
        Self {
            writer: BufWriter::new(output),
        }
    }

    pub fn write_all(&mut self, accounts: &HashMap<ClientId, Account>) -> Result<(), IoError> {
        writeln!(self.writer, "{}", ACCOUNT_HEADER_ROW)?;
        for (_, account) in accounts.iter() {
            writeln!(self.writer, "{}", account.to_csv_row())?;
        }
        self.writer.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::account::Account;
    use crate::model::transaction::{AccountTransactionInput, TransactionId, TransactionType};
    use crate::model::types::{Amount, ClientId, SCALE};
    use std::num::NonZeroU64;

    fn amount(val: f64) -> Amount {
        let scaled = (val * SCALE as f64).round() as u64;
        Amount(NonZeroU64::new(scaled).unwrap())
    }

    fn deposit(client: u16, tx: u32, val: f64) -> AccountTransactionInput {
        AccountTransactionInput {
            client_id: ClientId(client),
            transaction_type: TransactionType::Deposit { amount: amount(val) },
            transaction_id: TransactionId(tx),
        }
    }

    fn output_string(accounts: &HashMap<ClientId, Account>) -> String {
        let mut buf = Vec::new();
        {
            let mut writer = AccountWriter::new(&mut buf);
            writer.write_all(accounts).unwrap();
        }
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn writes_header_for_empty_accounts() {
        let accounts = HashMap::new();
        let output = output_string(&accounts);
        assert_eq!(output.trim(), "client,available,held,total,locked");
    }

    #[test]
    fn writes_single_account() {
        let mut accounts = HashMap::new();
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 1, 10.0)).unwrap();
        accounts.insert(ClientId(1), acct);
        let output = output_string(&accounts);
        let lines: Vec<&str> = output.trim().lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "client,available,held,total,locked");
        assert_eq!(lines[1], "1,10.0000,0.0000,10.0000,false");
    }

    #[test]
    fn writes_multiple_accounts() {
        let mut accounts = HashMap::new();
        let mut a1 = Account::new(ClientId(1));
        a1.handle_transaction(&deposit(1, 1, 5.5)).unwrap();
        accounts.insert(ClientId(1), a1);
        let mut a2 = Account::new(ClientId(2));
        a2.handle_transaction(&deposit(2, 2, 20.0)).unwrap();
        accounts.insert(ClientId(2), a2);
        let output = output_string(&accounts);
        let lines: Vec<&str> = output.trim().lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "client,available,held,total,locked");
        // Order not guaranteed, check both present
        let data: Vec<&&str> = lines[1..].iter().collect();
        assert!(data.contains(&&"1,5.5000,0.0000,5.5000,false"));
        assert!(data.contains(&&"2,20.0000,0.0000,20.0000,false"));
    }

    #[test]
    fn writes_frozen_account() {
        let mut accounts = HashMap::new();
        let mut acct = Account::new(ClientId(3));
        acct.handle_transaction(&deposit(3, 1, 10.0)).unwrap();
        acct.handle_transaction(&AccountTransactionInput {
            client_id: ClientId(3),
            transaction_type: TransactionType::Dispute,
            transaction_id: TransactionId(1),
        }).unwrap();
        acct.handle_transaction(&AccountTransactionInput {
            client_id: ClientId(3),
            transaction_type: TransactionType::Chargeback,
            transaction_id: TransactionId(1),
        }).unwrap();
        accounts.insert(ClientId(3), acct);
        let output = output_string(&accounts);
        let lines: Vec<&str> = output.trim().lines().collect();
        assert_eq!(lines[1], "3,0.0000,0.0000,0.0000,true");
    }

    #[test]
    fn writes_decimal_precision() {
        let mut accounts = HashMap::new();
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 1, 1.2345)).unwrap();
        accounts.insert(ClientId(1), acct);
        let output = output_string(&accounts);
        assert!(output.contains("1.2345"));
    }

    #[test]
    fn writes_to_generic_writer() {
        let mut buf: Vec<u8> = Vec::new();
        {
            let mut writer = AccountWriter::new(&mut buf);
            writer.write_all(&HashMap::new()).unwrap();
        }
        let output = String::from_utf8(buf).unwrap();
        assert!(output.starts_with("client,"));
    }
}