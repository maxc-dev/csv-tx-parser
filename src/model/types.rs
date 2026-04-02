use std::cmp::Ordering;
use std::num::NonZeroU32;
use std::ops::{AddAssign, SubAssign};

pub const DECIMAL_PRECISION: u8 = 4; // todo embed into the display

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct ClientId(pub u16);

#[derive(Debug, PartialEq, Eq, Hash, Copy, Clone)]
pub struct Amount(pub NonZeroU32);

#[derive(Debug, PartialEq, Eq, Hash)]
#[derive(PartialOrd)]
pub struct AvailableBalance(pub u128);

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct HeldBalance(pub u128);

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct TotalBalance(pub u128);

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct LockedBalance(pub u128);


impl AddAssign<Amount> for AvailableBalance {
    fn add_assign(&mut self, rhs: Amount) {
        self.0 += rhs.0.get() as u128;
    }
}

impl PartialEq<Amount> for AvailableBalance {
    fn eq(&self, other: &Amount) -> bool {
        self.0 == other.0.get() as u128
    }
}

impl PartialOrd<Amount> for AvailableBalance {
    fn partial_cmp(&self, other: &Amount) -> Option<Ordering> {
        Some(self.0.cmp(&(other.0.get() as u128)))
    }
}

impl SubAssign<Amount> for AvailableBalance {
    fn sub_assign(&mut self, rhs: Amount) {
        self.0 -= rhs.0.get() as u128;
    }
}

impl SubAssign<Amount> for TotalBalance {
    fn sub_assign(&mut self, rhs: Amount) {
        self.0 -= rhs.0.get() as u128;
    }
}

impl AddAssign<Amount> for TotalBalance {
    fn add_assign(&mut self, rhs: Amount) {
        self.0 += rhs.0.get() as u128;
    }
}

impl AddAssign<Amount> for LockedBalance {
    fn add_assign(&mut self, rhs: Amount) {
        self.0 += rhs.0.get() as u128;
    }
}

impl PartialEq<Amount> for LockedBalance {
    fn eq(&self, other: &Amount) -> bool {
        self.0 == other.0.get() as u128
    }
}

impl PartialOrd<Amount> for LockedBalance {
    fn partial_cmp(&self, other: &Amount) -> Option<Ordering> {
        Some(self.0.cmp(&(other.0.get() as u128)))
    }
}

impl SubAssign<Amount> for LockedBalance {
    fn sub_assign(&mut self, rhs: Amount) {
        self.0 -= rhs.0.get() as u128;
    }
}