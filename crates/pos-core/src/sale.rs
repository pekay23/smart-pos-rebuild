// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use serde::{Deserialize, Serialize};
use std::{error::Error, fmt};

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PricedLine {
    pub quantity: i64,
    pub unit_price_minor: i64,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SaleTotals {
    pub subtotal_minor: i64,
    pub discount_minor: i64,
    pub tax_minor: i64,
    pub total_minor: i64,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SaleLineAllocation {
    pub line_subtotal_minor: i64,
    pub discount_minor: i64,
    pub tax_minor: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalculationError {
    EmptySale,
    InvalidQuantity,
    NegativePrice,
    InvalidDiscount,
    InvalidTaxRate,
    Overflow,
}

impl fmt::Display for CalculationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::EmptySale => "A sale needs at least one item.",
            Self::InvalidQuantity => "Item quantities must be positive whole numbers.",
            Self::NegativePrice => "Item prices cannot be negative.",
            Self::InvalidDiscount => "The discount must be between zero and the subtotal.",
            Self::InvalidTaxRate => "The configured tax rate must be between 0 and 100 percent.",
            Self::Overflow => "The sale total exceeds the supported money range.",
        };
        formatter.write_str(message)
    }
}

impl Error for CalculationError {}

pub fn calculate_sale_totals(
    lines: &[PricedLine],
    discount_minor: i64,
    tax_rate_bps: i64,
) -> Result<SaleTotals, CalculationError> {
    if lines.is_empty() {
        return Err(CalculationError::EmptySale);
    }
    if tax_rate_bps < 0 || tax_rate_bps > 10_000 {
        return Err(CalculationError::InvalidTaxRate);
    }

    let mut subtotal = 0_i128;
    for line in lines {
        if line.quantity <= 0 {
            return Err(CalculationError::InvalidQuantity);
        }
        if line.unit_price_minor < 0 {
            return Err(CalculationError::NegativePrice);
        }
        subtotal = subtotal
            .checked_add(i128::from(line.quantity) * i128::from(line.unit_price_minor))
            .ok_or(CalculationError::Overflow)?;
    }

    let discount = i128::from(discount_minor);
    if discount < 0 || discount > subtotal {
        return Err(CalculationError::InvalidDiscount);
    }

    let taxable = subtotal - discount;
    let tax = taxable
        .checked_mul(i128::from(tax_rate_bps))
        .and_then(|amount| amount.checked_add(5_000))
        .ok_or(CalculationError::Overflow)?
        / 10_000;
    let total = taxable.checked_add(tax).ok_or(CalculationError::Overflow)?;

    Ok(SaleTotals {
        subtotal_minor: i64::try_from(subtotal).map_err(|_| CalculationError::Overflow)?,
        discount_minor,
        tax_minor: i64::try_from(tax).map_err(|_| CalculationError::Overflow)?,
        total_minor: i64::try_from(total).map_err(|_| CalculationError::Overflow)?,
    })
}

/// Allocate sale-level discount and tax back to item lines without losing minor units.
/// Remainders are assigned by largest fractional remainder, with line order as a stable tie-break.
pub fn allocate_sale_components(
    lines: &[PricedLine],
    totals: SaleTotals,
) -> Result<Vec<SaleLineAllocation>, CalculationError> {
    let subtotals = lines
        .iter()
        .map(|line| {
            if line.quantity <= 0 {
                return Err(CalculationError::InvalidQuantity);
            }
            if line.unit_price_minor < 0 {
                return Err(CalculationError::NegativePrice);
            }
            i64::try_from(i128::from(line.quantity) * i128::from(line.unit_price_minor))
                .map_err(|_| CalculationError::Overflow)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let subtotal_sum = subtotals.iter().try_fold(0_i64, |sum, value| {
        sum.checked_add(*value).ok_or(CalculationError::Overflow)
    })?;
    if subtotal_sum != totals.subtotal_minor
        || totals.discount_minor < 0
        || totals.discount_minor > subtotal_sum
        || totals.tax_minor < 0
    {
        return Err(CalculationError::InvalidDiscount);
    }
    let discounts = allocate_proportionally(&subtotals, totals.discount_minor)?;
    let taxable = subtotals
        .iter()
        .zip(&discounts)
        .map(|(subtotal, discount)| subtotal - discount)
        .collect::<Vec<_>>();
    let taxes = allocate_proportionally(&taxable, totals.tax_minor)?;
    Ok(subtotals
        .into_iter()
        .zip(discounts)
        .zip(taxes)
        .map(
            |((line_subtotal_minor, discount_minor), tax_minor)| SaleLineAllocation {
                line_subtotal_minor,
                discount_minor,
                tax_minor,
            },
        )
        .collect())
}

fn allocate_proportionally(weights: &[i64], total: i64) -> Result<Vec<i64>, CalculationError> {
    if total < 0 {
        return Err(CalculationError::InvalidDiscount);
    }
    let weight_sum = weights.iter().try_fold(0_i128, |sum, weight| {
        if *weight < 0 {
            return Err(CalculationError::InvalidDiscount);
        }
        sum.checked_add(i128::from(*weight))
            .ok_or(CalculationError::Overflow)
    })?;
    if weight_sum == 0 {
        return if total == 0 {
            Ok(vec![0; weights.len()])
        } else {
            Err(CalculationError::InvalidDiscount)
        };
    }
    let mut output = Vec::with_capacity(weights.len());
    let mut remainders = Vec::with_capacity(weights.len());
    let mut allocated = 0_i64;
    for (index, weight) in weights.iter().enumerate() {
        let scaled = i128::from(*weight) * i128::from(total);
        let part = i64::try_from(scaled / weight_sum).map_err(|_| CalculationError::Overflow)?;
        allocated = allocated
            .checked_add(part)
            .ok_or(CalculationError::Overflow)?;
        output.push(part);
        remainders.push((index, scaled % weight_sum));
    }
    let leftover = usize::try_from(total - allocated).map_err(|_| CalculationError::Overflow)?;
    remainders.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    if leftover > remainders.len() {
        return Err(CalculationError::Overflow);
    }
    for (index, _) in remainders.into_iter().take(leftover) {
        output[index] += 1;
    }
    Ok(output)
}

#[cfg(test)]
mod allocation_tests {
    use super::*;

    #[test]
    fn allocates_discount_and_tax_exactly_with_stable_remainders() {
        let lines = [
            PricedLine {
                quantity: 1,
                unit_price_minor: 100,
            },
            PricedLine {
                quantity: 1,
                unit_price_minor: 100,
            },
            PricedLine {
                quantity: 1,
                unit_price_minor: 100,
            },
        ];
        let totals = calculate_sale_totals(&lines, 1, 500).unwrap();
        let allocations = allocate_sale_components(&lines, totals).unwrap();
        assert_eq!(
            allocations
                .iter()
                .map(|line| line.discount_minor)
                .sum::<i64>(),
            1
        );
        assert_eq!(
            allocations.iter().map(|line| line.tax_minor).sum::<i64>(),
            totals.tax_minor
        );
        assert_eq!(allocations[0].discount_minor, 1);
        assert_eq!(allocations[1].discount_minor, 0);
    }

    #[test]
    fn rejects_component_totals_that_do_not_match_the_lines() {
        let lines = [PricedLine {
            quantity: 1,
            unit_price_minor: 10,
        }];
        let invalid = SaleTotals {
            subtotal_minor: 11,
            discount_minor: 0,
            tax_minor: 0,
            total_minor: 11,
        };
        assert_eq!(
            allocate_sale_components(&lines, invalid),
            Err(CalculationError::InvalidDiscount)
        );
    }
}
