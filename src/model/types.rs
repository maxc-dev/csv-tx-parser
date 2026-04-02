use std::cmp::Ordering;
use std::fmt::{Display, Formatter};
use std::marker::PhantomData;
use std::num::NonZeroU64;
use std::ops::{AddAssign, SubAssign};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NotSend(PhantomData<*const ()>);

impl NotSend {
    pub const fn new() -> Self {
        Self(PhantomData)
    }
}
#[derive(Debug, PartialEq, Eq, Hash, Copy, Clone)]
pub struct ClientId(pub u16);

#[derive(Debug, PartialEq, Eq, Hash, Copy, Clone)]
pub struct Amount(pub NonZeroU64);

#[derive(Debug, PartialOrd, PartialEq, Eq, Hash, Copy, Clone)]
pub struct Balance(pub u128);

pub const DECIMAL_PRECISION: u32 = 4;
pub const SCALE: u64 = 10u64.pow(DECIMAL_PRECISION);

impl Display for Balance {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let scale = SCALE as u128;
        let whole = self.0 / scale;
        let frac = self.0 % scale;
        write!(f, "{whole}.{frac:04}")
    }
}

impl Display for ClientId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Display for Amount {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl AddAssign<Amount> for Balance {
    fn add_assign(&mut self, rhs: Amount) {
        self.0 += rhs.0.get() as u128;
    }
}

impl PartialEq<Amount> for Balance {
    fn eq(&self, other: &Amount) -> bool {
        self.0 == other.0.get() as u128
    }
}

impl PartialOrd<Amount> for Balance {
    fn partial_cmp(&self, other: &Amount) -> Option<Ordering> {
        Some(self.0.cmp(&(other.0.get() as u128)))
    }
}

impl SubAssign<Amount> for Balance {
    fn sub_assign(&mut self, rhs: Amount) {
        self.0 = self
            .0
            .checked_sub(rhs.0.get() as u128)
            .expect("Balance underflow: subtraction would result in negative balance");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU64;

    fn amount(val: u64) -> Amount {
        Amount(NonZeroU64::new(val).unwrap())
    }

    // --- NotSend ---

    #[test]
    fn not_send_is_default() {
        let ns = NotSend::default();
        assert_eq!(ns, NotSend::new());
    }

    #[test]
    fn not_send_is_copy_clone() {
        let ns = NotSend::new();
        let ns2 = ns;
        let ns3 = ns.clone();
        assert_eq!(ns, ns2);
        assert_eq!(ns, ns3);
    }

    // --- Constants ---

    #[test]
    fn decimal_precision_is_4() {
        assert_eq!(DECIMAL_PRECISION, 4);
    }

    #[test]
    fn scale_is_10000() {
        assert_eq!(SCALE, 10_000);
    }

    // --- ClientId Display ---

    #[test]
    fn client_id_display() {
        assert_eq!(format!("{}", ClientId(0)), "0");
        assert_eq!(format!("{}", ClientId(1)), "1");
        assert_eq!(format!("{}", ClientId(u16::MAX)), "65535");
    }

    #[test]
    fn client_id_equality() {
        assert_eq!(ClientId(1), ClientId(1));
        assert_ne!(ClientId(1), ClientId(2));
    }

    // --- Amount Display ---

    #[test]
    fn amount_display() {
        assert_eq!(format!("{}", amount(10000)), "10000");
        assert_eq!(format!("{}", amount(1)), "1");
    }

    // --- Balance Display (decimal formatting) ---

    #[test]
    fn balance_display_zero() {
        assert_eq!(format!("{}", Balance(0)), "0.0000");
    }

    #[test]
    fn balance_display_whole_number() {
        assert_eq!(format!("{}", Balance(10000)), "1.0000");
        assert_eq!(format!("{}", Balance(20000)), "2.0000");
    }

    #[test]
    fn balance_display_fractional() {
        assert_eq!(format!("{}", Balance(15000)), "1.5000");
        assert_eq!(format!("{}", Balance(12345)), "1.2345");
        assert_eq!(format!("{}", Balance(1)), "0.0001");
    }

    #[test]
    fn balance_display_large() {
        assert_eq!(format!("{}", Balance(1_000_000_000)), "100000.0000");
    }

    // --- AddAssign<Amount> for Balance ---

    #[test]
    fn balance_add_assign_amount() {
        let mut b = Balance(0);
        b += amount(10000);
        assert_eq!(b, Balance(10000));
        b += amount(5000);
        assert_eq!(b, Balance(15000));
    }

    // --- SubAssign<Amount> for Balance ---

    #[test]
    fn balance_sub_assign_amount() {
        let mut b = Balance(20000);
        b -= amount(5000);
        assert_eq!(b, Balance(15000));
        b -= amount(15000);
        assert_eq!(b, Balance(0));
    }

    // --- PartialEq<Amount> for Balance ---

    #[test]
    fn balance_eq_amount() {
        assert_eq!(Balance(10000), amount(10000));
        assert!(!(Balance(10000) == amount(5000)));
        assert!(!(Balance(5000) == amount(10000)));
    }

    // --- PartialOrd<Amount> for Balance ---

    #[test]
    fn balance_partial_ord_amount() {
        assert!(Balance(10000) >= amount(10000));
        assert!(Balance(10001) > amount(10000));
        assert!(Balance(9999) < amount(10000));
    }

    #[test]
    fn balance_partial_cmp_amount() {
        assert_eq!(
            Balance(10000).partial_cmp(&amount(10000)),
            Some(Ordering::Equal)
        );
        assert_eq!(
            Balance(10001).partial_cmp(&amount(10000)),
            Some(Ordering::Greater)
        );
        assert_eq!(
            Balance(9999).partial_cmp(&amount(10000)),
            Some(Ordering::Less)
        );
    }

    // --- Balance PartialOrd (self) ---

    #[test]
    fn balance_ordering() {
        assert!(Balance(100) < Balance(200));
        assert!(Balance(200) > Balance(100));
        assert_eq!(Balance(100), Balance(100));
    }

    // --- Edge cases ---

    #[test]
    fn balance_add_assign_small_amount() {
        let mut b = Balance(0);
        b += amount(1); // 0.0001
        assert_eq!(b, Balance(1));
        assert_eq!(format!("{}", b), "0.0001");
    }

    #[test]
    fn balance_sub_assign_to_zero() {
        let mut b = Balance(1);
        b -= amount(1);
        assert_eq!(b, Balance(0));
    }

    #[test]
    #[should_panic(expected = "Balance underflow")]
    fn balance_sub_assign_underflow_panics() {
        let mut b = Balance(0);
        b -= amount(1);
    }

    #[test]
    #[should_panic(expected = "Balance underflow")]
    fn balance_sub_assign_underflow_by_one_panics() {
        let mut b = Balance(999);
        b -= amount(1000);
    }
}
