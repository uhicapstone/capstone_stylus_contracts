//! Insurance fee calculator for Uniswap v4 pools
//! Handles dynamic fee calculation based on volatility and other risk factors

#![cfg_attr(not(feature = "export-abi"), no_main)]
extern crate alloc;

use stylus_sdk::{
    alloy_primitives::{U256, FixedBytes},
    prelude::*,
};
use alloc::vec::Vec;

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
    // Calculates insurance fee based on:
    // - Pool volume (higher volume = lower fees)
    // - Price volatility (higher volatility = higher fees) 
    // - Historical IL (higher IL = higher fees)
    // - Trade size (larger trades = higher fees)
    pub fn calculate_insurance_fee(
        &mut self,
        pool_id: FixedBytes<32>,
        amount: U256,
        total_liquidity: U256,
        total_volume: U256,
        current_price: U256,
        timestamp: U256,
    ) -> Result<U256, Vec<u8>> {
        let volatility = self.calculate_volatility(pool_id, current_price, timestamp)?;
        let base_fee = U256::from(1_000_000_000_000_000u64); // 0.1% base fee
        
        // Volume discount for active pools
        let volume_multiplier = if total_volume > U256::ZERO {
            let volume_factor = U256::from(9e17); // 0.9x for high volume
            match total_volume.checked_mul(volume_factor)
                .and_then(|v| v.checked_div(total_volume + U256::from(1e18)))
                .and_then(|v| U256::from(1e18).checked_sub(v)) {
                Some(result) => result,
                None => return Err(Vec::from("Calculation error"))
            }
        } else {
            U256::from(1e18)
        };

        let volatility_multiplier = match volatility.checked_mul(U256::from(2))
            .and_then(|v| U256::from(1e18).checked_add(v))
            .and_then(|v| v.checked_div(U256::from(1e18))) {
            Some(result) => result,
            None => return Err(Vec::from("Calculation error"))
        };

        let historical_il = self.historical_il.get(pool_id);
        let il_multiplier = match historical_il.checked_mul(U256::from(3))
            .and_then(|v| U256::from(1e18).checked_add(v))
            .and_then(|v| v.checked_div(U256::from(1e18))) {
            Some(result) => result,
            None => return Err(Vec::from("Calculation error"))
        };

        let size_multiplier = if total_liquidity > U256::ZERO {
            match amount.checked_mul(U256::from(1e18))
                .and_then(|v| v.checked_div(total_liquidity))
                .and_then(|v| U256::from(1e18).checked_add(v)) {
                Some(result) => result,
                None => return Err(Vec::from("Calculation error"))
            }
        } else {
            U256::from(2e18)
        };

        let fee = match base_fee.checked_mul(volume_multiplier)
            .and_then(|v| v.checked_mul(volatility_multiplier))
            .and_then(|v| v.checked_mul(il_multiplier))
            .and_then(|v| v.checked_mul(size_multiplier))
            .and_then(|v| v.checked_div(U256::from(1e18)))
            .and_then(|v| v.checked_div(U256::from(1e18)))
            .and_then(|v| v.checked_div(U256::from(1e18))) {
            Some(result) => result,
            None => return Err(Vec::from("Calculation error"))
        };

        Ok(fee)
    }

    // Uses EMA of price returns to estimate volatility
    // Updates price history every hour and applies decay factor
    pub fn calculate_volatility(
        &mut self,
        pool_id: FixedBytes<32>,
        current_price: U256,
        timestamp: U256,
    ) -> Result<U256, Vec<u8>> {
        let last_update = self.price_data_timestamp.get(pool_id);
        let update_index = self.price_update_index.get(pool_id);
        
        // Update price history if an hour has passed
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

        // Calculate returns with decay factor
        for i in 0..30 {
            if let Some(element) = price_history.getter(i) {
                let historical_price = element.get();
                
                if historical_price > U256::ZERO {
                    let price_diff = if last_valid_price > historical_price {
                        last_valid_price - historical_price
                    } else {
                        historical_price - last_valid_price
                    };
                    
                    let return_value = match price_diff.checked_mul(U256::from(1e18))
                        .and_then(|v| v.checked_div(historical_price)) {
                        Some(result) => result,
                        None => continue
                    };

                    let decay_factor = match U256::from(95).pow(U256::from(i))
                        .checked_div(U256::from(100).pow(U256::from(i))) {
                        Some(result) => result,
                        None => continue
                    };
                    
                    sum_squared_returns = match return_value.checked_mul(return_value)
                        .and_then(|v| v.checked_mul(decay_factor))
                        .and_then(|v| v.checked_div(U256::from(1e18)))
                        .and_then(|v| sum_squared_returns.checked_add(v)) {
                        Some(result) => result,
                        None => continue
                    };

                    valid_points = valid_points + U256::from(1);
                    last_valid_price = historical_price;
                }
            }
        }

        if valid_points == U256::ZERO {
            return Ok(U256::ZERO);
        }

        // Convert to annualized volatility 
        let avg_squared_return = match sum_squared_returns.checked_mul(U256::from(1e18))
            .and_then(|v| v.checked_div(valid_points)) {
            Some(result) => result,
            None => return Err(Vec::from("Calculation error"))
        };
        
        let volatility = match Self::sqrt(avg_squared_return)
            .checked_mul(U256::from(Self::sqrt(U256::from(365 * 24))))
            .and_then(|v| v.checked_div(U256::from(1e9))) {
            Some(result) => result,
            None => return Err(Vec::from("Calculation error"))
        };

        Ok(volatility)
    }
}
