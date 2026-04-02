use crate::io::reader::{CsvRow, TransactionReader};
use crate::model::account::{ACCOUNT_HEADER_ROW, Account, AccountError};
use crate::model::transaction::{AccountTransactionInput, TransactionParseError};
use crate::model::types::{ClientId, NotSend};
use crate::transaction_processor::TransactionProcessorError::{
    ClientAccountError, CsvReaderError, LineParseError,
};
use csv::Error as CsvError;
use std::collections::HashMap;
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
                Ok(_) => {
                    tracing::debug!("Transaction processed successfully: {id}");
                }
                Err(err) => {
                    tracing::warn!("Transaction {id} error: {err}");
                    if self.fail_file_on_error {
                        return Err(err);
                    }
                }
            }
        }
        Ok(())
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
                tracing::error!("Encountered an error during processing: {error}");
                Err(ClientAccountError(error))
            }
        }
    }

    pub fn output_accounts_csv(&self) {
        let len = self.accounts.len();
        tracing::info!("Printing {:?} accounts", len);

        println!("{}", ACCOUNT_HEADER_ROW);
        for (_, account) in self.accounts.iter() {
            println!("{}", account.to_csv_row());
        }

        tracing::info!("Printed {:?} accounts in {} time", len, 1); // todo add a time here
    }
}
