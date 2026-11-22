// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

mod sale;

pub use sale::{
    allocate_sale_components, calculate_sale_totals, CalculationError, PricedLine,
    SaleLineAllocation, SaleTotals,
};
