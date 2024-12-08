#![cfg_attr(all(not(feature = "std"), not(feature = "export-abi")), no_main)]
extern crate alloc;

use stylus_sdk::{
    alloy_primitives::{U256, FixedBytes}, 
    prelude::*,
    alloy_sol_types::sol,
    stylus_proc::{public, sol_storage, SolidityError},
};

sol! {
    #[derive(Debug)]
    error CalculationError();
    
    #[derive(Debug)] 
    error InvalidInput();
}

#[derive(SolidityError, Debug)]
pub enum Error {
    /// Math calculation error
    CalculationError(CalculationError),
    /// Invalid input parameters 
    InvalidInput(InvalidInput)
}

sol_storage! {
    #[entrypoint]
    pub struct InsuranceCalculator {
        mapping(bytes32 => uint256) historical_il;
        mapping(bytes32 => uint256) default_flash_fee_multiplier;
    }
}

#[public]
impl InsuranceCalculator {
    /// Calculates insurance fee for a trade
    pub fn calculate_insurance_fee(
        &self,
        pool_id: FixedBytes<32>,
        amount: U256,
        total_liquidity: U256,
        total_volume: U256,
        current_price: U256,
        timestamp: U256,
    ) -> Result<U256, Error> {
        // Base fee for insurance, fixed at 0.1%
        let base_fee = U256::from(100_000_000_000_000_000u64);

        // Volume multiplier: decreases fee if volume is high
        let volume_multiplier = if total_volume > U256::ZERO {
            let factor = total_volume
                .checked_mul(U256::from(900_000_000_000_000_000u64))
                .ok_or(Error::CalculationError(CalculationError{}))? // Ensure no overflow
                .checked_div(total_volume.checked_add(U256::from(1_000_000_000_000_000_000u64))
                .ok_or(Error::CalculationError(CalculationError{}))?)
                .ok_or(Error::CalculationError(CalculationError{}))?; // Normalize by volume + 1e18
            U256::from(100_000_000_000_000_000u64)
                .checked_add(factor)
                .ok_or(Error::CalculationError(CalculationError{}))? // Add factor
        } else {
            U256::from(1_000_000_000_000_000_000u64) // Default to 1.0 if no volume
        };

        // Historical IL multiplier: higher IL means higher risk, thus higher fees
        let historical_il = self.historical_il.get(pool_id);
        let il_multiplier = historical_il
            .checked_mul(U256::from(3_000_000_000_000_000_000u64))
            .ok_or(Error::CalculationError(CalculationError{}))? // Amplify IL effect
            .checked_add(U256::from(1_000_000_000_000_000_000u64))
            .ok_or(Error::CalculationError(CalculationError{}))?; // Add baseline multiplier

        // Size multiplier: larger trades pay proportionally higher fees
        let size_multiplier = if total_liquidity > U256::ZERO {
            amount
                .checked_mul(U256::from(1_000_000_000_000_000_000u64))
                .ok_or(Error::CalculationError(CalculationError{}))? // Scale trade size
                .checked_div(total_liquidity)
                .ok_or(Error::CalculationError(CalculationError{}))? // Normalize by pool liquidity
                .checked_add(U256::from(1_000_000_000_000_000_000u64))
                .ok_or(Error::CalculationError(CalculationError{}))? // Baseline multiplier
        } else {
            U256::from(2_000_000_000_000_000_000u64) // Default if no liquidity
        };

        // Final fee = base * volume * IL * size, scaled down for precision
        let fee = base_fee
            .checked_mul(volume_multiplier)
            .ok_or(Error::CalculationError(CalculationError{}))?
            .checked_mul(il_multiplier)
            .ok_or(Error::CalculationError(CalculationError{}))?
            .checked_mul(size_multiplier)
            .ok_or(Error::CalculationError(CalculationError{}))?
            .checked_div(U256::from(1_000_000_000_000_000_000u64).pow(U256::from(3)))
            .ok_or(Error::CalculationError(CalculationError{}))?;

        Ok(fee)
    }

    /// Calculates flash loan fee for a borrowing
    pub fn calculate_flash_loan_fee(
        &self,
        amount: U256,
        total_liquidity: U256,
        utilization_rate: U256,
        default_history: U256,
    ) -> Result<U256, Error> {
        // Base fee for flash loans, fixed at 0.05%
        let base_fee = U256::from(500_000_000_000_000u64);

        // Utilization multiplier: scales up fee when pool usage is high
        let utilization_multiplier = utilization_rate
            .checked_mul(U256::from(2))
            .ok_or(Error::CalculationError(CalculationError{}))? // Amplify by 2x
            .checked_add(U256::from(1_000_000_000_000_000_000u64))
            .ok_or(Error::CalculationError(CalculationError{}))? // Add baseline multiplier
            .checked_div(U256::from(1_000_000_000_000_000_000u64))
            .ok_or(Error::CalculationError(CalculationError{}))?; // Normalize

        // Liquidity multiplier: reduces fee when liquidity is high
        let liquidity_multiplier = if total_liquidity > U256::ZERO {
            U256::from(1_000_000_000_000_000_000u64)
                .checked_div(total_liquidity.checked_add(U256::from(1_000_000_000_000_000_000u64))
                .ok_or(Error::CalculationError(CalculationError{}))?)
                .ok_or(Error::CalculationError(CalculationError{}))? // Adjust by available liquidity
                .checked_add(U256::from(1_000_000_000_000_000_000u64))
                .ok_or(Error::CalculationError(CalculationError{}))? // Baseline multiplier
        } else {
            U256::from(2_000_000_000_000_000_000u64) // Default multiplier 2.0 * 1e18
        };

        // Historical multiplier: default adjustment for past performance
        let historical_multiplier = U256::from(1_000_000_000_000_000_000u64)
            .checked_add(default_history)
            .ok_or(Error::CalculationError(CalculationError{}))?; // Add historical adjustment

        // Final fee = base * utilization * liquidity * historical, scaled down for precision
        let fee = base_fee
            .checked_mul(utilization_multiplier)
            .ok_or(Error::CalculationError(CalculationError{}))?
            .checked_mul(liquidity_multiplier)
            .ok_or(Error::CalculationError(CalculationError{}))?
            .checked_mul(historical_multiplier)
            .ok_or(Error::CalculationError(CalculationError{}))?
            .checked_div(U256::from(1_000_000_000_000_000_000u64).pow(U256::from(3)))
            .ok_or(Error::CalculationError(CalculationError{}))?;

        // Scale by the loan amount
        let final_fee = fee
            .checked_mul(amount)
            .ok_or(Error::CalculationError(CalculationError{}))?
            .checked_div(U256::from(1_000_000_000_000_000_000u64))
            .ok_or(Error::CalculationError(CalculationError{}))?; // Scale fee by amount

        Ok(final_fee)
    }
}
