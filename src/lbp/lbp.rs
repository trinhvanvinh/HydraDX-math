use core::convert::{TryFrom, TryInto};
use primitive_types::U256;

use crate::{
    ensure, to_balance, to_lbp_weight, to_u256, MathError,
    MathError::{Overflow, ZeroDuration, ZeroReserve, ZeroWeight},
};

use core::convert::From;
use num_traits::Zero;

use crate::types::{Balance, FixedBalance, LBPWeight, HYDRA_ONE};

/// Calculating spot price given reserve of selling asset and reserve of buying asset.
/// Formula : BUY_RESERVE * AMOUNT / SELL_RESERVE
///
/// - `in_reserve` - reserve amount of selling asset
/// - `out_reserve` - reserve amount of buying asset
/// - `in_weight` - pool weight of selling asset
/// - `out_Weight` - pool weight of buying asset
/// - `amount` - amount
///
/// Returns None in case of error
pub fn calculate_spot_price(
    in_reserve: Balance,
    out_reserve: Balance,
    in_weight: LBPWeight,
    out_weight: LBPWeight,
    amount: Balance,
) -> Result<Balance, MathError> {
    // If any is 0 - let's not progress any further.
    ensure!(in_reserve != 0, ZeroReserve);

    if amount == 0 || out_reserve == 0 {
        return to_balance!(0);
    }

    let (amount, out_reserve, in_reserve, out_weight, in_weight) =
        to_u256!(amount, out_reserve, in_reserve, out_weight, in_weight);

    let spot_price = amount
        .checked_mul(out_reserve)
        .ok_or(Overflow)?
        .checked_mul(in_weight)
        .ok_or(Overflow)?
        .checked_div(in_reserve.checked_mul(out_weight).ok_or(Overflow)?)
        .ok_or(Overflow)?;

    to_balance!(spot_price)
}

fn convert_to_fixed(value: Balance) -> FixedBalance {
    if value == Balance::from(1u32) {
        return FixedBalance::from_num(1);
    }

    // Unwrap is safer here
    let f = value.checked_div(HYDRA_ONE).unwrap();
    let r = value - (f.checked_mul(HYDRA_ONE).unwrap());
    FixedBalance::from_num(f) + (FixedBalance::from_num(r) / HYDRA_ONE)
}

fn convert_from_fixed(value: FixedBalance) -> Option<Balance> {
    let w: Balance = value.int().to_num();
    let frac = value.frac();
    let frac: Balance = frac.checked_mul_int(HYDRA_ONE)?.int().to_num();
    let r = w.checked_mul(HYDRA_ONE)?.checked_add(frac)?;
    Some(r)
}

fn round_up_fixed(value: FixedBalance) -> Result<FixedBalance, MathError> {
    let prec = FixedBalance::from_num(0.00000000001);
    value.checked_add(prec).ok_or(Overflow)
}

#[macro_export]
macro_rules! to_fixed_balance{
    ($($x:expr),+) => (
        {($(convert_to_fixed($x)),+)}
    );
}

#[macro_export]
macro_rules! to_balance_from_fixed {
    ($x:expr) => {
        convert_from_fixed($x).ok_or(Overflow)
    };
}

/// Calculating selling price given reserve of selling asset and reserve of buying asset.
///
/// - `in_reserve` - reserve amount of selling asset
/// - `out_reserve` - reserve amount of buying asset
/// - `in_weight` - pool weight of selling asset
/// - `out_weight` - pool weight of buying asset
/// - `amount` - amount
///
/// Returns None in case of error
pub fn calculate_out_given_in(
    in_reserve: Balance,
    out_reserve: Balance,
    in_weight: LBPWeight,
    out_weight: LBPWeight,
    amount: Balance,
) -> Result<Balance, MathError> {
    ensure!(out_weight != 0, ZeroWeight);
    ensure!(in_weight != 0, ZeroWeight);
    ensure!(out_reserve != 0, ZeroReserve);
    ensure!(in_reserve != 0, ZeroWeight);

    if amount.is_zero() {
        return Ok(0u128);
    }

    let (in_weight, out_weight, amount, in_reserve, out_reserve) =
        to_fixed_balance!(in_weight as u128, out_weight as u128, amount, in_reserve, out_reserve);

    // We are correctly rounding this down
    let weight_ratio = in_weight.checked_div(out_weight).ok_or(Overflow)?;

    // We round this up
    // This ratio being closer to one (i.e. rounded up) minimizes the impact of the asset
    // that was sold to the pool, i.e. 'amount'
    let new_in_reserve = in_reserve.checked_add(amount).ok_or(Overflow)?;
    let ir = round_up_fixed(in_reserve.checked_div(new_in_reserve).ok_or(Overflow)?)?;

    let t1 = amount.checked_add(in_reserve).ok_or(Overflow)?;
    if ir.checked_mul(t1).ok_or(Overflow)? < in_reserve {
        return Err(Overflow);
    }

    let ir = crate::transcendental::pow(ir, weight_ratio).map_err(|_| Overflow)?;

    // We round this up
    let new_out_reserve_calc = round_up_fixed(out_reserve.checked_mul(ir).ok_or(Overflow)?)?;

    let r = out_reserve.checked_sub(new_out_reserve_calc).ok_or(Overflow)?;

    let new_out_reserve = out_reserve.checked_sub(r).unwrap();

    if new_out_reserve < new_out_reserve_calc {
        return Err(Overflow);
    }

    let out_delta = out_reserve.checked_sub(new_out_reserve).ok_or(Overflow)?;
    let out_delta_calc = out_reserve.checked_sub(new_out_reserve_calc).ok_or(Overflow)?;

    if out_delta > out_delta_calc {
        return Err(Overflow);
    }

    to_balance_from_fixed!(r)
}

/// Calculating buying price given reserve of selling asset and reserve of buying asset.
/// Formula :
///
/// - `in_reserve` - reserve amount of selling asset
/// - `out_reserve` - reserve amount of buying asset
/// - `in_weight` - pool weight of selling asset
/// - `out_weight` - pool weight of buying asset
/// - `amount` - buy amount
///
/// Returns None in case of error
pub fn calculate_in_given_out(
    in_reserve: Balance,
    out_reserve: Balance,
    in_weight: LBPWeight,
    out_weight: LBPWeight,
    amount: Balance,
) -> Result<Balance, MathError> {
    let (in_weight, out_weight, amount, in_reserve, out_reserve) =
        to_fixed_balance!(in_weight as u128, out_weight as u128, amount, in_reserve, out_reserve);

    let weight_ratio = round_up_fixed(out_weight.checked_div(in_weight).ok_or(Overflow)?)?;

    let new_out_reserve = out_reserve.checked_sub(amount).ok_or(Overflow)?;
    // We are correctly rounding this down
    let y = out_reserve.checked_div(new_out_reserve).ok_or(Overflow)?;

    let y1: FixedBalance = crate::transcendental::pow(y, weight_ratio).map_err(|_| Overflow)?;

    let y2 = y1.checked_sub(FixedBalance::from_num(1u128)).ok_or(Overflow)?;

    let r = in_reserve.checked_mul(y2).ok_or(Overflow)?;

    let amount_in = round_up_fixed(r)?;

    to_balance_from_fixed!(amount_in)
}

/// Calculating weight at any given block in an interval using linear interpolation.
///
/// - `start_x` - beginning of an interval
/// - `end_x` - end of an interval
/// - `start_y` - initial weight
/// - `end_y` - final weight
/// - `at` - block number at which to calculate the weight
pub fn calculate_linear_weights<BlockNumber: num_traits::CheckedSub + TryInto<u32> + TryInto<u128>>(
    start_x: BlockNumber,
    end_x: BlockNumber,
    start_y: LBPWeight,
    end_y: LBPWeight,
    at: BlockNumber,
) -> Result<LBPWeight, MathError> {
    let d1 = end_x.checked_sub(&at).ok_or(Overflow)?;
    let d2 = at.checked_sub(&start_x).ok_or(Overflow)?;
    let dx = end_x.checked_sub(&start_x).ok_or(Overflow)?;

    let dx: u32 = dx.try_into().map_err(|_| Overflow)?;
    // if dx fits into u32, d1 and d2 fit into u128
    let d1: u128 = d1.try_into().map_err(|_| Overflow)?;
    let d2: u128 = d2.try_into().map_err(|_| Overflow)?;

    ensure!(dx != 0, ZeroDuration);

    let (start_y, end_y, d1, d2) = to_u256!(start_y, end_y, d1, d2);

    let left_part = start_y.checked_mul(d1).ok_or(Overflow)?;
    let right_part = end_y.checked_mul(d2).ok_or(Overflow)?;
    let result = (left_part.checked_add(right_part).ok_or(Overflow)?)
        .checked_div(dx.into())
        .ok_or(Overflow)?;

    to_lbp_weight!(result)
}
