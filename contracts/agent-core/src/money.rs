//! Money math. Every function uses checked u128 arithmetic and rounds in a
//! fixed direction so that parts always add up to the whole (E20, I2).

use crate::Amount;

pub const BPS: u128 = 10_000;

/// Multiply by basis points, rounding down. Panics only on overflow, which
/// cannot happen for amounts below 2^114 (token supplies are far smaller).
pub fn mul_bps(amount: Amount, bps: u16) -> Amount {
    amount
        .checked_mul(bps as u128)
        .expect("overflow in mul_bps")
        / BPS
}

/// Protocol fee on every sale and share buy (1%), taken from the seller's
/// upfront part and credited to the fee account (planned for mainnet; live on
/// testnet with fake money). `fee <= now` for any terms the contract accepts.
pub const PROTOCOL_FEE_BPS: u16 = 100;

pub fn protocol_fee(amount: Amount) -> Amount {
    mul_bps(amount, PROTOCOL_FEE_BPS)
}

/// Split a sale price into (paid to the seller now, held in earn-out escrow).
/// The seller's part rounds down, so any rounding remainder goes to escrow
/// and `now + escrow == price` always (E20).
pub fn split_price(price: Amount, earnout_bps: u16) -> (Amount, Amount) {
    let seller_bps = (BPS as u16)
        .checked_sub(earnout_bps)
        .expect("earnout_bps > 100%");
    let now = mul_bps(price, seller_bps);
    let escrow = price.checked_sub(now).expect("split underflow");
    (now, escrow)
}

/// Split an incoming payment into (owner's revenue, agent treasury) by the
/// owner's limits. The treasury part rounds down, the owner gets the rest, so
/// the two always add up to the payment exactly (feature 12, `prop_split_revenue`).
pub fn split_revenue(amount: Amount, treasury_bps: u16) -> (Amount, Amount) {
    let treasury = mul_bps(amount, treasury_bps.min(BPS as u16));
    (amount - treasury, treasury)
}

/// Earn-out released by one counted post-sale payment: `share_bps` of the
/// payment, never more than what is left in escrow.
pub fn release_for_payment(payment: Amount, share_bps: u16, remaining: Amount) -> Amount {
    mul_bps(payment, share_bps).min(remaining)
}

/// Result of closing an earn-out at the end of its window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Finalize {
    /// Extra paid to the seller so the seller's total reaches the floor.
    pub seller_topup: Amount,
    /// Returned to the buyer.
    pub buyer_refund: Amount,
}

/// At window end: the seller is topped up to `floor_bps` of the escrow
/// ("silence pays the floor"); everything else goes back to the buyer.
pub fn finalize(escrow: Amount, released: Amount, floor_bps: u16) -> Finalize {
    let remaining = escrow.checked_sub(released).expect("released > escrow");
    let floor = mul_bps(escrow, floor_bps);
    let seller_topup = floor.saturating_sub(released).min(remaining);
    Finalize {
        seller_topup,
        buyer_refund: remaining - seller_topup,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::USD;

    #[test]
    fn split_fixed_world() {
        assert_eq!(split_price(1_000 * USD, 3_000), (700 * USD, 300 * USD));
    }

    #[test]
    fn e20_split_exact_odd_price() {
        let price = 999_999_999; // 999.999999
        let (now, escrow) = split_price(price, 3_000);
        assert_eq!(now + escrow, price);
        assert_eq!(now, 699_999_999);
        assert_eq!(escrow, 300_000_000);
    }

    #[test]
    fn s11_revenue_split_25() {
        assert_eq!(split_revenue(25 * USD, 2_000), (20 * USD, 5 * USD));
        assert_eq!(split_revenue(7, 3_333), (5, 2));
        assert_eq!(split_revenue(7, 0), (7, 0));
    }

    #[test]
    fn finalize_silence_pays_floor() {
        let f = finalize(300 * USD, 0, 2_000);
        assert_eq!(f.seller_topup, 60 * USD);
        assert_eq!(f.buyer_refund, 240 * USD);
    }

    #[test]
    fn finalize_partial_and_full() {
        let f = finalize(300 * USD, 50 * USD, 2_000);
        assert_eq!((f.seller_topup, f.buyer_refund), (10 * USD, 240 * USD));
        let f = finalize(300 * USD, 300 * USD, 2_000);
        assert_eq!((f.seller_topup, f.buyer_refund), (0, 0));
    }
}
