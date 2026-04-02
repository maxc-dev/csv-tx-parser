use crate::model::transaction::{
    AccountTransactionInput, TransactionId, TransactionState, TransactionType,
};
use crate::model::types::{Amount, Balance, ClientId, NotSend};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Default)]
pub enum AccountState {
    #[default]
    Active,
    Frozen,
}

impl AccountState {
    pub fn is_locked(&self) -> bool {
        matches!(self, AccountState::Frozen)
    }
}

#[derive(Debug, Error)]
pub enum AccountError {
    #[error("Account {client_id:?} is frozen, rejecting transaction {transaction_id:?}")]
    AccountFrozen {
        client_id: ClientId,
        transaction_id: TransactionId,
    },

    #[error(
        "Transaction {transaction_id:?} references an incorrect client ID, expected: {expected:?}, actual: {actual:?}"
    )]
    InvalidClientId {
        expected: ClientId,
        actual: ClientId,
        transaction_id: TransactionId,
    },

    #[error("Insufficient available funds: have {available:?}, need {requested:?}")]
    InsufficientAvailableFunds {
        available: Balance,
        requested: Amount,
    },

    #[error("Insufficient held funds: have {held:?}, need {requested:?}")]
    InsufficientHeldFunds { held: Balance, requested: Amount },

    #[error("Transaction {transaction_id:?} not found")]
    TransactionNotFound { transaction_id: TransactionId },

    #[error(
        "Invalid state transition {transaction_id:?} type {transaction_type:?} in state {current_state:?}"
    )]
    InvalidStateTransition {
        transaction_id: TransactionId,
        transaction_type: TransactionType,
        current_state: Option<TransactionState>,
    },
}

pub struct Account {
    client_id: ClientId,
    available_balance: Balance,
    held_balance: Balance,
    total_balance: Balance,
    transaction_states: HashMap<TransactionId, TransactionState>,
    transaction_amounts: HashMap<TransactionId, Amount>,
    account_state: AccountState,
    _ns: NotSend,
}

pub(crate) const ACCOUNT_HEADER_ROW: &str = "client,available,held,total,locked";

impl Account {
    pub fn new(client_id: ClientId) -> Self {
        Self {
            client_id,
            available_balance: Balance(0),
            held_balance: Balance(0),
            total_balance: Balance(0),
            transaction_states: HashMap::new(),
            transaction_amounts: HashMap::new(),
            account_state: AccountState::Active,
            _ns: NotSend::new(),
        }
    }

    pub fn handle_transaction(
        &mut self,
        transaction: &AccountTransactionInput,
    ) -> Result<(), AccountError> {
        let transaction_id = transaction.transaction_id;
        let transaction_type = transaction.transaction_type;

        if transaction.client_id != self.client_id {
            return Err(AccountError::InvalidClientId {
                transaction_id,
                expected: self.client_id,
                actual: transaction.client_id,
            });
        }

        if self.account_state.is_locked() {
            tracing::warn!(
                "Transaction attempted on frozen account {:?} for transaction ID {:?}",
                self.client_id,
                transaction_id,
            );
            return Err(AccountError::AccountFrozen {
                client_id: self.client_id,
                transaction_id,
            });
        }

        let current_state = self.get_transaction_state(&transaction_id);
        let state_transition_result = self.handle_transaction_state_transition(
            transaction_id,
            transaction_type,
            current_state,
        )?;

        self.transaction_states
            .insert(transaction_id, state_transition_result);
        tracing::debug!(
            "Handled transaction ID {:?} type {:?} for account {:?}",
            transaction_id,
            transaction_type,
            self.client_id
        );

        Ok(())
    }

    fn handle_transaction_state_transition(
        &mut self,
        transaction_id: TransactionId,
        transaction_type: TransactionType,
        current_state: Option<TransactionState>,
    ) -> Result<TransactionState, AccountError> {
        match (transaction_type, current_state) {
            (TransactionType::Deposit { amount }, None) => {
                self.handle_deposit(transaction_id, amount)
            }
            (TransactionType::Withdraw { amount }, None) => {
                self.handle_withdrawal(transaction_id, amount)
            }
            (TransactionType::Dispute, Some(TransactionState::DepositComplete)) => {
                self.handle_dispute(transaction_id)
            }
            (TransactionType::Resolve, Some(TransactionState::Disputed)) => {
                self.handle_resolve(transaction_id)
            }
            (TransactionType::Chargeback, Some(TransactionState::Disputed)) => {
                self.handle_chargeback(transaction_id)
            }
            _ => {
                tracing::warn!(
                    "Invalid transaction type {:?} for transaction ID {:?}, current state: {:?}",
                    transaction_type,
                    transaction_id,
                    current_state,
                );
                Err(AccountError::InvalidStateTransition {
                    transaction_id,
                    transaction_type,
                    current_state,
                })
            }
        }
    }

    fn get_transaction_state(
        &self,
        transaction_id: &TransactionId,
    ) -> Option<TransactionState> {
        self.transaction_states.get(transaction_id).copied()
    }

    fn get_amount_from_transaction_id(&self, id: TransactionId) -> Result<&Amount, AccountError> {
        self.transaction_amounts
            .get(&id)
            .ok_or(AccountError::TransactionNotFound { transaction_id: id })
    }

    fn handle_deposit(
        &mut self,
        id: TransactionId,
        amount: Amount,
    ) -> Result<TransactionState, AccountError> {
        self.available_balance += amount;
        self.total_balance += amount;
        self.transaction_amounts.insert(id, amount);
        Ok(TransactionState::DepositComplete)
    }

    fn handle_withdrawal(
        &mut self,
        id: TransactionId,
        amount: Amount,
    ) -> Result<TransactionState, AccountError> {
        if self.available_balance < amount {
            return Err(AccountError::InsufficientAvailableFunds {
                available: self.available_balance,
                requested: amount,
            });
        }
        self.available_balance -= amount;
        self.total_balance -= amount;
        self.transaction_amounts.insert(id, amount);
        Ok(TransactionState::WithdrawalComplete)
    }

    fn handle_dispute(
        &mut self,
        transaction_id: TransactionId,
    ) -> Result<TransactionState, AccountError> {
        let amount = *self.get_amount_from_transaction_id(transaction_id)?;
        if self.available_balance < amount {
            return Err(AccountError::InsufficientAvailableFunds {
                available: self.available_balance,
                requested: amount,
            });
        }
        self.available_balance -= amount;
        self.held_balance += amount;
        Ok(TransactionState::Disputed)
    }

    fn handle_resolve(
        &mut self,
        transaction_id: TransactionId,
    ) -> Result<TransactionState, AccountError> {
        let amount = *self.get_amount_from_transaction_id(transaction_id)?;
        if self.held_balance < amount {
            return Err(AccountError::InsufficientHeldFunds {
                held: self.held_balance,
                requested: amount,
            });
        }
        self.held_balance -= amount;
        self.available_balance += amount;
        Ok(TransactionState::Resolved)
    }

    fn handle_chargeback(
        &mut self,
        transaction_id: TransactionId,
    ) -> Result<TransactionState, AccountError> {
        let amount = *self.get_amount_from_transaction_id(transaction_id)?;
        if self.held_balance < amount {
            return Err(AccountError::InsufficientHeldFunds {
                held: self.held_balance,
                requested: amount,
            });
        }
        self.held_balance -= amount;
        self.total_balance -= amount;
        self.freeze_account();
        Ok(TransactionState::Chargeback)
    }

    fn freeze_account(&mut self) {
        self.account_state = AccountState::Frozen;
        tracing::info!("Account {:?} frozen", self.client_id);
    }

    pub fn to_csv_row(&self) -> String {
        format!(
            "{},{},{},{},{}",
            self.client_id.0,
            self.available_balance,
            self.held_balance,
            self.total_balance,
            self.account_state.is_locked(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::types::{Amount, ClientId, SCALE};
    use std::num::NonZeroU64;

    fn amount(val: f64) -> Amount {
        let scaled = (val * SCALE as f64).round() as u64;
        Amount(NonZeroU64::new(scaled).unwrap())
    }

    fn tx_id(id: u32) -> TransactionId {
        TransactionId(id)
    }

    fn deposit(tx: u32, val: f64) -> AccountTransactionInput {
        AccountTransactionInput {
            client_id: ClientId(1),
            transaction_type: TransactionType::Deposit {
                amount: amount(val),
            },
            transaction_id: tx_id(tx),
        }
    }

    fn withdrawal(tx: u32, val: f64) -> AccountTransactionInput {
        AccountTransactionInput {
            client_id: ClientId(1),
            transaction_type: TransactionType::Withdraw {
                amount: amount(val),
            },
            transaction_id: tx_id(tx),
        }
    }

    fn dispute(tx: u32) -> AccountTransactionInput {
        AccountTransactionInput {
            client_id: ClientId(1),
            transaction_type: TransactionType::Dispute,
            transaction_id: tx_id(tx),
        }
    }

    fn resolve(tx: u32) -> AccountTransactionInput {
        AccountTransactionInput {
            client_id: ClientId(1),
            transaction_type: TransactionType::Resolve,
            transaction_id: tx_id(tx),
        }
    }

    fn chargeback(tx: u32) -> AccountTransactionInput {
        AccountTransactionInput {
            client_id: ClientId(1),
            transaction_type: TransactionType::Chargeback,
            transaction_id: tx_id(tx),
        }
    }

    fn bal(val: f64) -> u128 {
        (val * SCALE as f64).round() as u128
    }

    // --- Deposit ---

    #[test]
    fn deposit_increases_available_and_total() {
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 10.0)).unwrap();
        assert_eq!(acct.available_balance.0, bal(10.0));
        assert_eq!(acct.total_balance.0, bal(10.0));
        assert_eq!(acct.held_balance.0, 0);
    }

    #[test]
    fn multiple_deposits_accumulate() {
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 10.0)).unwrap();
        acct.handle_transaction(&deposit(2, 20.5)).unwrap();
        assert_eq!(acct.available_balance.0, bal(30.5));
        assert_eq!(acct.total_balance.0, bal(30.5));
    }

    #[test]
    fn deposit_with_four_decimal_places() {
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 1.2345)).unwrap();
        assert_eq!(acct.available_balance.0, bal(1.2345));
    }

    // --- Withdrawal ---

    #[test]
    fn withdrawal_decreases_available_and_total() {
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 20.0)).unwrap();
        acct.handle_transaction(&withdrawal(2, 8.0)).unwrap();
        assert_eq!(acct.available_balance.0, bal(12.0));
        assert_eq!(acct.total_balance.0, bal(12.0));
    }

    #[test]
    fn withdrawal_insufficient_funds_returns_error() {
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 5.0)).unwrap();
        let result = acct.handle_transaction(&withdrawal(2, 10.0));
        assert!(matches!(
            result,
            Err(AccountError::InsufficientAvailableFunds { .. })
        ));
        assert_eq!(acct.available_balance.0, bal(5.0));
        assert_eq!(acct.total_balance.0, bal(5.0));
    }

    #[test]
    fn withdrawal_exact_balance_succeeds() {
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 5.0)).unwrap();
        acct.handle_transaction(&withdrawal(2, 5.0)).unwrap();
        assert_eq!(acct.available_balance.0, 0);
        assert_eq!(acct.total_balance.0, 0);
    }

    // --- Dispute ---

    #[test]
    fn dispute_moves_available_to_held() {
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 10.0)).unwrap();
        acct.handle_transaction(&dispute(1)).unwrap();
        assert_eq!(acct.available_balance.0, 0);
        assert_eq!(acct.held_balance.0, bal(10.0));
        assert_eq!(acct.total_balance.0, bal(10.0));
    }

    #[test]
    fn dispute_insufficient_available_returns_error() {
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 10.0)).unwrap();
        acct.handle_transaction(&withdrawal(2, 8.0)).unwrap();
        let result = acct.handle_transaction(&dispute(1));
        assert!(matches!(
            result,
            Err(AccountError::InsufficientAvailableFunds { .. })
        ));
        assert_eq!(acct.available_balance.0, bal(2.0));
        assert_eq!(acct.held_balance.0, 0);
    }

    #[test]
    fn dispute_nonexistent_tx_returns_error() {
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 10.0)).unwrap();
        let result = acct.handle_transaction(&dispute(99));
        assert!(matches!(
            result,
            Err(AccountError::InvalidStateTransition { .. })
        ));
    }

    #[test]
    fn dispute_on_withdrawal_returns_error() {
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 10.0)).unwrap();
        acct.handle_transaction(&withdrawal(2, 5.0)).unwrap();
        let result = acct.handle_transaction(&dispute(2));
        assert!(matches!(
            result,
            Err(AccountError::InvalidStateTransition { .. })
        ));
    }

    #[test]
    fn double_dispute_returns_error() {
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 10.0)).unwrap();
        acct.handle_transaction(&dispute(1)).unwrap();
        let result = acct.handle_transaction(&dispute(1));
        assert!(matches!(
            result,
            Err(AccountError::InvalidStateTransition { .. })
        ));
    }

    // --- Resolve ---

    #[test]
    fn resolve_moves_held_back_to_available() {
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 10.0)).unwrap();
        acct.handle_transaction(&dispute(1)).unwrap();
        acct.handle_transaction(&resolve(1)).unwrap();
        assert_eq!(acct.available_balance.0, bal(10.0));
        assert_eq!(acct.held_balance.0, 0);
        assert_eq!(acct.total_balance.0, bal(10.0));
    }

    #[test]
    fn resolve_without_dispute_returns_error() {
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 10.0)).unwrap();
        let result = acct.handle_transaction(&resolve(1));
        assert!(matches!(
            result,
            Err(AccountError::InvalidStateTransition { .. })
        ));
    }

    #[test]
    fn resolve_nonexistent_tx_returns_error() {
        let mut acct = Account::new(ClientId(1));
        let result = acct.handle_transaction(&resolve(99));
        assert!(matches!(
            result,
            Err(AccountError::InvalidStateTransition { .. })
        ));
    }

    // --- Chargeback ---

    #[test]
    fn chargeback_decreases_held_and_total_and_freezes() {
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 10.0)).unwrap();
        acct.handle_transaction(&dispute(1)).unwrap();
        acct.handle_transaction(&chargeback(1)).unwrap();
        assert_eq!(acct.available_balance.0, 0);
        assert_eq!(acct.held_balance.0, 0);
        assert_eq!(acct.total_balance.0, 0);
        assert!(acct.account_state.is_locked());
    }

    #[test]
    fn chargeback_without_dispute_returns_error() {
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 10.0)).unwrap();
        let result = acct.handle_transaction(&chargeback(1));
        assert!(matches!(
            result,
            Err(AccountError::InvalidStateTransition { .. })
        ));
        assert!(!acct.account_state.is_locked());
    }

    #[test]
    fn chargeback_nonexistent_tx_returns_error() {
        let mut acct = Account::new(ClientId(1));
        let result = acct.handle_transaction(&chargeback(99));
        assert!(matches!(
            result,
            Err(AccountError::InvalidStateTransition { .. })
        ));
        assert!(!acct.account_state.is_locked());
    }

    // --- Frozen account ---

    #[test]
    fn frozen_account_rejects_deposit() {
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 10.0)).unwrap();
        acct.handle_transaction(&dispute(1)).unwrap();
        acct.handle_transaction(&chargeback(1)).unwrap();
        let result = acct.handle_transaction(&deposit(2, 5.0));
        assert!(matches!(result, Err(AccountError::AccountFrozen { .. })));
    }

    #[test]
    fn frozen_account_rejects_withdrawal() {
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 20.0)).unwrap();
        acct.handle_transaction(&deposit(2, 10.0)).unwrap();
        acct.handle_transaction(&dispute(1)).unwrap();
        acct.handle_transaction(&chargeback(1)).unwrap();
        let result = acct.handle_transaction(&withdrawal(3, 5.0));
        assert!(matches!(result, Err(AccountError::AccountFrozen { .. })));
    }

    // --- Client ID mismatch ---

    #[test]
    fn wrong_client_id_returns_error() {
        let mut acct = Account::new(ClientId(1));
        let wrong_client = AccountTransactionInput {
            client_id: ClientId(2),
            transaction_type: TransactionType::Deposit {
                amount: amount(10.0),
            },
            transaction_id: tx_id(1),
        };
        let result = acct.handle_transaction(&wrong_client);
        assert!(matches!(result, Err(AccountError::InvalidClientId { .. })));
    }

    // --- Duplicate tx ID ---

    #[test]
    fn duplicate_deposit_tx_id_rejected() {
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 10.0)).unwrap();
        let result = acct.handle_transaction(&deposit(1, 5.0));
        assert!(matches!(
            result,
            Err(AccountError::InvalidStateTransition { .. })
        ));
        assert_eq!(acct.available_balance.0, bal(10.0));
    }

    // --- CSV output ---

    #[test]
    fn to_csv_row_format() {
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 1.5)).unwrap();
        let row = acct.to_csv_row();
        assert_eq!(row, "1,1.5000,0.0000,1.5000,false");
    }

    #[test]
    fn to_csv_row_frozen() {
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 10.0)).unwrap();
        acct.handle_transaction(&dispute(1)).unwrap();
        acct.handle_transaction(&chargeback(1)).unwrap();
        let row = acct.to_csv_row();
        assert_eq!(row, "1,0.0000,0.0000,0.0000,true");
    }

    // --- Full lifecycle ---

    #[test]
    fn deposit_dispute_resolve_cycle() {
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 50.0)).unwrap();
        acct.handle_transaction(&dispute(1)).unwrap();
        assert_eq!(acct.available_balance.0, 0);
        assert_eq!(acct.held_balance.0, bal(50.0));
        acct.handle_transaction(&resolve(1)).unwrap();
        assert_eq!(acct.available_balance.0, bal(50.0));
        assert_eq!(acct.held_balance.0, 0);
        assert!(!acct.account_state.is_locked());
    }

    #[test]
    fn partial_dispute_with_remaining_balance() {
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 30.0)).unwrap();
        acct.handle_transaction(&deposit(2, 20.0)).unwrap();
        acct.handle_transaction(&dispute(1)).unwrap();
        assert_eq!(acct.available_balance.0, bal(20.0));
        assert_eq!(acct.held_balance.0, bal(30.0));
        assert_eq!(acct.total_balance.0, bal(50.0));
    }

    // --- State machine: invalid transitions ---

    #[test]
    fn resolve_on_deposit_complete_is_invalid() {
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 10.0)).unwrap();
        let result = acct.handle_transaction(&resolve(1));
        assert!(matches!(
            result,
            Err(AccountError::InvalidStateTransition { .. })
        ));
        assert_eq!(acct.available_balance.0, bal(10.0));
        assert_eq!(acct.held_balance.0, 0);
    }

    #[test]
    fn chargeback_on_deposit_complete_is_invalid() {
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 10.0)).unwrap();
        let result = acct.handle_transaction(&chargeback(1));
        assert!(matches!(
            result,
            Err(AccountError::InvalidStateTransition { .. })
        ));
        assert!(!acct.account_state.is_locked());
        assert_eq!(acct.available_balance.0, bal(10.0));
    }

    #[test]
    fn dispute_on_withdrawal_complete_is_invalid() {
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 10.0)).unwrap();
        acct.handle_transaction(&withdrawal(2, 5.0)).unwrap();
        let result = acct.handle_transaction(&dispute(2));
        assert!(matches!(
            result,
            Err(AccountError::InvalidStateTransition { .. })
        ));
        assert_eq!(acct.available_balance.0, bal(5.0));
        assert_eq!(acct.held_balance.0, 0);
    }

    #[test]
    fn resolve_on_withdrawal_complete_is_invalid() {
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 10.0)).unwrap();
        acct.handle_transaction(&withdrawal(2, 5.0)).unwrap();
        let result = acct.handle_transaction(&resolve(2));
        assert!(matches!(
            result,
            Err(AccountError::InvalidStateTransition { .. })
        ));
    }

    #[test]
    fn chargeback_on_withdrawal_complete_is_invalid() {
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 10.0)).unwrap();
        acct.handle_transaction(&withdrawal(2, 5.0)).unwrap();
        let result = acct.handle_transaction(&chargeback(2));
        assert!(matches!(
            result,
            Err(AccountError::InvalidStateTransition { .. })
        ));
    }

    #[test]
    fn deposit_on_existing_tx_id_is_invalid() {
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 10.0)).unwrap();
        let result = acct.handle_transaction(&deposit(1, 20.0));
        assert!(matches!(
            result,
            Err(AccountError::InvalidStateTransition { .. })
        ));
        assert_eq!(acct.available_balance.0, bal(10.0));
    }

    #[test]
    fn withdrawal_on_existing_deposit_tx_id_is_invalid() {
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 10.0)).unwrap();
        let result = acct.handle_transaction(&withdrawal(1, 5.0));
        assert!(matches!(
            result,
            Err(AccountError::InvalidStateTransition { .. })
        ));
        assert_eq!(acct.available_balance.0, bal(10.0));
    }

    #[test]
    fn dispute_on_resolved_tx_is_invalid() {
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 10.0)).unwrap();
        acct.handle_transaction(&dispute(1)).unwrap();
        acct.handle_transaction(&resolve(1)).unwrap();
        // Tx is now Resolved — cannot re-dispute
        let result = acct.handle_transaction(&dispute(1));
        assert!(matches!(
            result,
            Err(AccountError::InvalidStateTransition { .. })
        ));
        assert_eq!(acct.available_balance.0, bal(10.0));
        assert_eq!(acct.held_balance.0, 0);
    }

    #[test]
    fn resolve_on_chargebacked_tx_is_invalid() {
        let mut acct = Account::new(ClientId(2));
        acct.handle_transaction(&AccountTransactionInput {
            client_id: ClientId(2),
            transaction_type: TransactionType::Deposit {
                amount: amount(10.0),
            },
            transaction_id: tx_id(1),
        })
        .unwrap();
        acct.handle_transaction(&AccountTransactionInput {
            client_id: ClientId(2),
            transaction_type: TransactionType::Dispute,
            transaction_id: tx_id(1),
        })
        .unwrap();
        acct.handle_transaction(&AccountTransactionInput {
            client_id: ClientId(2),
            transaction_type: TransactionType::Chargeback,
            transaction_id: tx_id(1),
        })
        .unwrap();
        // Account is frozen, so any further tx returns AccountFrozen
        let result = acct.handle_transaction(&AccountTransactionInput {
            client_id: ClientId(2),
            transaction_type: TransactionType::Resolve,
            transaction_id: tx_id(1),
        });
        assert!(matches!(result, Err(AccountError::AccountFrozen { .. })));
    }

    #[test]
    fn dispute_on_disputed_tx_is_invalid() {
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 10.0)).unwrap();
        acct.handle_transaction(&dispute(1)).unwrap();
        let result = acct.handle_transaction(&dispute(1));
        assert!(matches!(
            result,
            Err(AccountError::InvalidStateTransition { .. })
        ));
    }

    #[test]
    fn resolve_on_resolved_tx_is_invalid() {
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 10.0)).unwrap();
        acct.handle_transaction(&dispute(1)).unwrap();
        acct.handle_transaction(&resolve(1)).unwrap();
        let result = acct.handle_transaction(&resolve(1));
        assert!(matches!(
            result,
            Err(AccountError::InvalidStateTransition { .. })
        ));
    }

    #[test]
    fn chargeback_on_resolved_tx_is_invalid() {
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 10.0)).unwrap();
        acct.handle_transaction(&dispute(1)).unwrap();
        acct.handle_transaction(&resolve(1)).unwrap();
        let result = acct.handle_transaction(&chargeback(1));
        assert!(matches!(
            result,
            Err(AccountError::InvalidStateTransition { .. })
        ));
    }

    // --- State machine: frozen account blocks all transition types ---

    #[test]
    fn frozen_account_rejects_dispute() {
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 10.0)).unwrap();
        acct.handle_transaction(&deposit(2, 5.0)).unwrap();
        acct.handle_transaction(&dispute(1)).unwrap();
        acct.handle_transaction(&chargeback(1)).unwrap();
        let result = acct.handle_transaction(&dispute(2));
        assert!(matches!(result, Err(AccountError::AccountFrozen { .. })));
    }

    #[test]
    fn frozen_account_rejects_resolve() {
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 10.0)).unwrap();
        acct.handle_transaction(&dispute(1)).unwrap();
        acct.handle_transaction(&chargeback(1)).unwrap();
        let result = acct.handle_transaction(&resolve(1));
        assert!(matches!(result, Err(AccountError::AccountFrozen { .. })));
    }

    #[test]
    fn resolve_insufficient_held_funds_returns_error() {
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 10.0)).unwrap();
        acct.handle_transaction(&dispute(1)).unwrap();
        // Manually reduce held_balance to simulate insufficient held funds
        acct.held_balance = Balance(0);
        let result = acct.handle_transaction(&resolve(1));
        assert!(matches!(
            result,
            Err(AccountError::InsufficientHeldFunds { .. })
        ));
    }

    #[test]
    fn chargeback_insufficient_held_funds_returns_error() {
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 10.0)).unwrap();
        acct.handle_transaction(&dispute(1)).unwrap();
        // Manually reduce held_balance to simulate insufficient held funds
        acct.held_balance = Balance(0);
        let result = acct.handle_transaction(&chargeback(1));
        assert!(matches!(
            result,
            Err(AccountError::InsufficientHeldFunds { .. })
        ));
    }

    #[test]
    fn frozen_account_rejects_chargeback() {
        let mut acct = Account::new(ClientId(1));
        acct.handle_transaction(&deposit(1, 10.0)).unwrap();
        acct.handle_transaction(&dispute(1)).unwrap();
        acct.handle_transaction(&chargeback(1)).unwrap();
        let result = acct.handle_transaction(&chargeback(1));
        assert!(matches!(result, Err(AccountError::AccountFrozen { .. })));
    }
}
