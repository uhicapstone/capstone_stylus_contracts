//! Insurance fee calculator for Uniswap v4 pools
//! Handles dynamic fee calculation based on volatility and other risk factors

#![cfg_attr(all(not(feature = "std"), not(feature = "export-abi")), no_main)]
extern crate alloc;

use stylus_sdk::{
    // evm,
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

    // event InsuranceFeeCalculated(
    //     bytes32 indexed pool_id,
    //     uint256 amount,
    //     uint256 fee
    // );

    // event VolatilityUpdated(
    //     bytes32 indexed pool_id,
    //     uint256 volatility
    // );
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
        mapping(bytes32 => uint256) last_calculation_time;
        mapping(bytes32 => uint256) volatility_accumulator;
        mapping(bytes32 => uint256) update_count;
        mapping(bytes32 => uint256) price_data_timestamp;
        mapping(bytes32 => uint256[30]) price_history;
        mapping(bytes32 => uint256) price_update_index;
        mapping(bytes32 => uint256) historical_il;
    }
}

impl InsuranceCalculator {
    // Basic Newton's method sqrt implementation
    fn sqrt(x: U256) -> U256 {
        if x == U256::ZERO {
            return U256::ZERO;
        }
        
        let mut z = (x + U256::from(1)) / U256::from(2);
        let mut y = x;
        
        while z < y {
            y = z;
            z = (x / z + z) / U256::from(2);
        }
        
        y
    }
}

#[public]
impl InsuranceCalculator {
    /// Calculates insurance fee based on:
    /// - Pool volume (higher volume = lower fees)
    /// - Price volatility (higher volatility = higher fees) 
    /// - Historical IL (higher IL = higher fees)
    /// - Trade size (larger trades = higher fees)
    ///
    /// # Arguments
    /// * `pool_id` - Unique identifier for the pool
    /// * `amount` - Trade amount
    /// * `total_liquidity` - Total pool liquidity
    /// * `total_volume` - Total pool volume
    /// * `current_price` - Current asset price
    /// * `timestamp` - Current block timestamp
    ///
    /// # Errors
    /// Returns `Error::CalculationError` on math overflow
    /// Returns `Error::InvalidInput` on invalid parameters
    pub fn calculate_insurance_fee(
        &mut self,
        pool_id: FixedBytes<32>,
        amount: U256,
        total_liquidity: U256,
        total_volume: U256,
        current_price: U256,
        timestamp: U256,
    ) -> Result<U256, Error> {
        // Original logic remains the same, just updated error handling
        let volatility = self.calculate_volatility(pool_id, current_price, timestamp)?;
        let base_fee = U256::from(1_000_000_000_000_000u64); // 0.1% base fee
        
        let volume_multiplier = if total_volume > U256::ZERO {
            let volume_factor = U256::from(9e17); // 0.9x for high volume
            total_volume.checked_mul(volume_factor)
                .and_then(|v| v.checked_div(total_volume + U256::from(1e18)))
                .and_then(|v| U256::from(1e18).checked_sub(v))
                .ok_or(Error::CalculationError(CalculationError{}))?
        } else {
            U256::from(1e18)
        };

        let volatility_multiplier = volatility.checked_mul(U256::from(2))
            .and_then(|v| U256::from(1e18).checked_add(v))
            .and_then(|v| v.checked_div(U256::from(1e18)))
            .ok_or(Error::CalculationError(CalculationError{}))?;

        let historical_il = self.historical_il.get(pool_id);
        let il_multiplier = historical_il.checked_mul(U256::from(3))
            .and_then(|v| U256::from(1e18).checked_add(v))
            .and_then(|v| v.checked_div(U256::from(1e18)))
            .ok_or(Error::CalculationError(CalculationError{}))?;

        let size_multiplier = if total_liquidity > U256::ZERO {
            amount.checked_mul(U256::from(1e18))
                .and_then(|v| v.checked_div(total_liquidity))
                .and_then(|v| U256::from(1e18).checked_add(v))
                .ok_or(Error::CalculationError(CalculationError{}))?
        } else {
            U256::from(2e18)
        };

        let fee = base_fee.checked_mul(volume_multiplier)
            .and_then(|v| v.checked_mul(volatility_multiplier))
            .and_then(|v| v.checked_mul(il_multiplier))
            .and_then(|v| v.checked_mul(size_multiplier))
            .and_then(|v| v.checked_div(U256::from(1e18)))
            .and_then(|v| v.checked_div(U256::from(1e18)))
            .and_then(|v| v.checked_div(U256::from(1e18)))
            .ok_or(Error::CalculationError(CalculationError{}))?;

        // Emit event
        // evm::log(InsuranceFeeCalculated {
        //     pool_id,
        //     amount,
        //     fee
        // });

        Ok(fee)
    }

    /// Uses EMA of price returns to estimate volatility
    /// Updates price history every hour and applies decay factor
    ///
    /// # Arguments
    /// * `pool_id` - Unique identifier for the pool
    /// * `current_price` - Current asset price
    /// * `timestamp` - Current block timestamp
    ///
    /// # Errors
    /// Returns `Error::CalculationError` on math overflow
    pub fn calculate_volatility(
        &mut self,
        pool_id: FixedBytes<32>,
        current_price: U256,
        timestamp: U256,
    ) -> Result<U256, Error> {
        // Original logic remains the same, just updated error handling
        let last_update = self.price_data_timestamp.get(pool_id);
        let update_index = self.price_update_index.get(pool_id);
        
        if timestamp >= last_update + U256::from(3600) {
            let new_index = (update_index + U256::from(1)) % U256::from(30);
            let mut price_history = self.price_history.setter(pool_id);
            let index: usize = new_index.try_into().unwrap_or(0);
            if let Some(mut element) = price_history.setter(index) {
                element.set(current_price);
            }
            self.price_update_index.setter(pool_id).set(new_index);
            self.price_data_timestamp.setter(pool_id).set(timestamp);
        }

        let mut sum_squared_returns = U256::ZERO;
        let mut valid_points = U256::ZERO;
        let mut last_valid_price = current_price;

        let price_history = self.price_history.getter(pool_id);

        for i in 0..30 {
            if let Some(element) = price_history.getter(i) {
                let historical_price = element.get();
                
                if historical_price > U256::ZERO {
                    let price_diff = if last_valid_price > historical_price {
                        last_valid_price - historical_price
                    } else {
                        historical_price - last_valid_price
                    };
                    
                    let return_value = price_diff.checked_mul(U256::from(1e18))
                        .and_then(|v| v.checked_div(historical_price))
                        .ok_or(Error::CalculationError(CalculationError{}))?;

                    let decay_factor = U256::from(95).pow(U256::from(i))
                        .checked_div(U256::from(100).pow(U256::from(i)))
                        .ok_or(Error::CalculationError(CalculationError{}))?;
                    
                    sum_squared_returns = return_value.checked_mul(return_value)
                        .and_then(|v| v.checked_mul(decay_factor))
                        .and_then(|v| v.checked_div(U256::from(1e18)))
                        .and_then(|v| sum_squared_returns.checked_add(v))
                        .ok_or(Error::CalculationError(CalculationError{}))?;

                    valid_points = valid_points + U256::from(1);
                    last_valid_price = historical_price;
                }
            }
        }

        if valid_points == U256::ZERO {
            return Ok(U256::ZERO);
        }

        let avg_squared_return = sum_squared_returns.checked_mul(U256::from(1e18))
            .and_then(|v| v.checked_div(valid_points))
            .ok_or(Error::CalculationError(CalculationError{}))?;
        
        let volatility = Self::sqrt(avg_squared_return)
            .checked_mul(U256::from(Self::sqrt(U256::from(365 * 24))))
            .and_then(|v| v.checked_div(U256::from(1e9)))
            .ok_or(Error::CalculationError(CalculationError{}))?;

        // Emit event
        // evm::log(VolatilityUpdated {
        //     pool_id,
        //     volatility
        // });

        Ok(volatility)
    }

    /// Pure calculation function that doesn't modify state
    pub fn calculate_flash_loan_fee(
        &self,
        amount: U256,
        total_liquidity: U256,
        utilization_rate: U256,
        default_history: U256,
    ) -> Result<U256, Error> {
        // Base fee of 0.05%
        let base_fee = U256::from(500_000_000_000_000u64); 

        // Pure calculations without state modifications
        let utilization_multiplier = if utilization_rate > U256::ZERO {
            utilization_rate
                .checked_mul(U256::from(2e18))
                .and_then(|v| v.checked_div(U256::from(1e18)))
                .and_then(|v| v.checked_add(U256::from(1e18)))
                .ok_or(Error::CalculationError(CalculationError{}))?
        } else {
            U256::from(1e18)
        };

        let liquidity_multiplier = if total_liquidity > U256::ZERO {
            let liquidity_factor = U256::from(1e18)
                .checked_mul(U256::from(1e18))
                .and_then(|v| v.checked_div(total_liquidity.checked_add(U256::from(1e18)).unwrap_or(U256::from(1))))
                .ok_or(Error::CalculationError(CalculationError{}))?;
            U256::from(1e18)
                .checked_add(liquidity_factor)
                .ok_or(Error::CalculationError(CalculationError{}))?
        } else {
            U256::from(2e18)
        };

        let default_multiplier = U256::from(1e18)
            .checked_add(default_history)
            .ok_or(Error::CalculationError(CalculationError{}))?;

        // Calculate final fee
        let fee = base_fee
            .checked_mul(utilization_multiplier)
            .and_then(|v| v.checked_mul(liquidity_multiplier))
            .and_then(|v| v.checked_mul(default_multiplier))
            .and_then(|v| v.checked_div(U256::from(1e18)))
            .and_then(|v| v.checked_div(U256::from(1e18)))
            .and_then(|v| v.checked_div(U256::from(1e18)))
            .ok_or(Error::CalculationError(CalculationError{}))?;

        // Scale fee by amount
        let final_fee = fee
            .checked_mul(amount)
            .and_then(|v| v.checked_div(U256::from(1e18)))
            .ok_or(Error::CalculationError(CalculationError{}))?;

        Ok(final_fee)
    }
}
