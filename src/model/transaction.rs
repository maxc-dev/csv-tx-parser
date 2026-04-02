use crate::io::reader::CsvRow;
use crate::model::types::{Amount, ClientId, SCALE};
use std::num::NonZeroU64;
use thiserror::Error;

#[derive(Copy, Clone, Debug)]
pub enum TransactionType {
    Withdraw { amount: Amount },
    Deposit { amount: Amount },
    Dispute,
    Resolve,
    Chargeback,
}

#[derive(Debug, Copy, Clone)]
pub(crate) enum TransactionState {
    DepositComplete,
    WithdrawalComplete,
    Disputed,
    Resolved,
    Chargeback,
}

#[derive(Debug, Eq, Hash, PartialEq, Copy, Clone)]
pub struct TransactionId(pub u32);

pub(crate) struct AccountTransactionInput {
    pub(crate) client_id: ClientId,
    pub(crate) transaction_type: TransactionType,
    pub(crate) transaction_id: TransactionId,
}

#[derive(Debug, Error)]
pub enum TransactionParseError {
    #[error("Unknown transaction type: {0}")]
    UnknownType(String),
    #[error("Invalid amount {0:?}")]
    InvalidAmount(Option<f64>),
    #[error("Invalid zero amount")]
    ZeroAmount,
}

const WITHDRAWAL: &str = "withdrawal";
const DEPOSIT: &str = "deposit";
const DISPUTE: &str = "dispute";
const RESOLVE: &str = "resolve";
const CHARGEBACK: &str = "chargeback";

impl AccountTransactionInput {
    pub fn create(raw: CsvRow) -> Result<AccountTransactionInput, TransactionParseError> {
        let transaction_type = match raw.transaction_type.as_str() {
            WITHDRAWAL => Ok(TransactionType::Withdraw {
                amount: Amount::from(raw.amount)?,
            }),
            DEPOSIT => Ok(TransactionType::Deposit {
                amount: Amount::from(raw.amount)?,
            }),
            DISPUTE => Ok(TransactionType::Dispute),
            RESOLVE => Ok(TransactionType::Resolve),
            CHARGEBACK => Ok(TransactionType::Chargeback),
            _ => Err(TransactionParseError::UnknownType(raw.transaction_type)),
        }?;

        Ok(Self {
            client_id: ClientId(raw.client_id),
            transaction_type,
            transaction_id: TransactionId(raw.transaction_id),
        })
    }
}

impl Amount {
    pub fn from(raw: Option<f64>) -> Result<Self, TransactionParseError> {
        match raw {
            Some(amount) => {
                let scaled = (amount * SCALE as f64).round() as u64;
                match NonZeroU64::new(scaled) {
                    Some(non_zero_amount) => Ok(Amount(non_zero_amount)),
                    None => Err(TransactionParseError::ZeroAmount),
                }
            }
            None => Err(TransactionParseError::InvalidAmount(raw)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::reader::CsvRow;

    fn csv_row(tx_type: &str, client: u16, tx: u32, amount: Option<f64>) -> CsvRow {
        CsvRow {
            transaction_type: tx_type.to_string(),
            client_id: client,
            transaction_id: tx,
            amount,
        }
    }

    // --- Amount::from tests ---

    #[test]
    fn amount_from_valid_value() {
        let amt = Amount::from(Some(1.5)).unwrap();
        assert_eq!(amt.0.get(), 15000);
    }

    #[test]
    fn amount_from_whole_number() {
        let amt = Amount::from(Some(2.0)).unwrap();
        assert_eq!(amt.0.get(), 20000);
    }

    #[test]
    fn amount_from_four_decimal_places() {
        let amt = Amount::from(Some(1.2345)).unwrap();
        assert_eq!(amt.0.get(), 12345);
    }

    #[test]
    fn amount_from_smallest_value() {
        let amt = Amount::from(Some(0.0001)).unwrap();
        assert_eq!(amt.0.get(), 1);
    }

    #[test]
    fn amount_from_zero_returns_error() {
        let result = Amount::from(Some(0.0));
        assert!(matches!(result, Err(TransactionParseError::ZeroAmount)));
    }

    #[test]
    fn amount_from_none_returns_error() {
        let result = Amount::from(None);
        assert!(matches!(
            result,
            Err(TransactionParseError::InvalidAmount(None))
        ));
    }

    #[test]
    fn amount_from_rounds_correctly() {
        // 1.00005 should round to 1.0001 (10001)
        let amt = Amount::from(Some(1.00005)).unwrap();
        assert_eq!(amt.0.get(), 10001);
    }

    // --- AccountTransactionInput::create tests ---

    #[test]
    fn create_deposit() {
        let input = AccountTransactionInput::create(csv_row("deposit", 1, 100, Some(5.0))).unwrap();
        assert_eq!(input.client_id, ClientId(1));
        assert_eq!(input.transaction_id, TransactionId(100));
        assert!(
            matches!(input.transaction_type, TransactionType::Deposit { amount } if amount.0.get() == 50000)
        );
    }

    #[test]
    fn create_withdrawal() {
        let input =
            AccountTransactionInput::create(csv_row("withdrawal", 2, 200, Some(3.5))).unwrap();
        assert_eq!(input.client_id, ClientId(2));
        assert_eq!(input.transaction_id, TransactionId(200));
        assert!(
            matches!(input.transaction_type, TransactionType::Withdraw { amount } if amount.0.get() == 35000)
        );
    }

    #[test]
    fn create_dispute() {
        let input = AccountTransactionInput::create(csv_row("dispute", 1, 100, None)).unwrap();
        assert!(matches!(input.transaction_type, TransactionType::Dispute));
    }

    #[test]
    fn create_resolve() {
        let input = AccountTransactionInput::create(csv_row("resolve", 1, 100, None)).unwrap();
        assert!(matches!(input.transaction_type, TransactionType::Resolve));
    }

    #[test]
    fn create_chargeback() {
        let input = AccountTransactionInput::create(csv_row("chargeback", 1, 100, None)).unwrap();
        assert!(matches!(
            input.transaction_type,
            TransactionType::Chargeback
        ));
    }

    #[test]
    fn create_unknown_type_returns_error() {
        let result = AccountTransactionInput::create(csv_row("refund", 1, 100, Some(1.0)));
        assert!(matches!(result, Err(TransactionParseError::UnknownType(s)) if s == "refund"));
    }

    #[test]
    fn create_deposit_missing_amount_returns_error() {
        let result = AccountTransactionInput::create(csv_row("deposit", 1, 100, None));
        assert!(matches!(
            result,
            Err(TransactionParseError::InvalidAmount(None))
        ));
    }

    #[test]
    fn create_withdrawal_missing_amount_returns_error() {
        let result = AccountTransactionInput::create(csv_row("withdrawal", 1, 100, None));
        assert!(matches!(
            result,
            Err(TransactionParseError::InvalidAmount(None))
        ));
    }

    #[test]
    fn create_deposit_zero_amount_returns_error() {
        let result = AccountTransactionInput::create(csv_row("deposit", 1, 100, Some(0.0)));
        assert!(matches!(result, Err(TransactionParseError::ZeroAmount)));
    }

    #[test]
    fn create_dispute_ignores_amount() {
        let input =
            AccountTransactionInput::create(csv_row("dispute", 1, 100, Some(999.0))).unwrap();
        assert!(matches!(input.transaction_type, TransactionType::Dispute));
    }

    #[test]
    fn create_preserves_client_and_tx_ids() {
        let input = AccountTransactionInput::create(csv_row("deposit", 65535, u32::MAX, Some(1.0)))
            .unwrap();
        assert_eq!(input.client_id, ClientId(65535));
        assert_eq!(input.transaction_id, TransactionId(u32::MAX));
    }
}
