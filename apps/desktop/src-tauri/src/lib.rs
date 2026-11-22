// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

mod accounting;
mod attendance;
mod audit;
mod backups;
mod cash_drawer;
mod cash_locations;
mod commands;
mod db;
mod discounts;
mod expense_attribution;
mod expense_corrections;
mod expense_reports;
mod inventory_counts;
mod media;
mod models;
mod passkeys;
mod payment_options;
mod payment_requests;
mod product_labels;
mod purchase_orders;
mod purchase_reports;
mod refunds;
mod roles;
mod sales;
mod staff;
mod stations;
mod store_settings;
mod supplier_billing;
mod updater;

use tauri::Manager;

pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let state = db::AppState::open(app.handle()).map_err(std::io::Error::other)?;
            app.manage(state);
            app.manage(updater::UpdateState::new());
            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(30));
                let state = handle.state::<db::AppState>();
                if let Ok(db) = state.db.lock() {
                    if let Err(error) = attendance::reconcile(&db, chrono::Utc::now()) {
                        eprintln!("attendance reconciliation failed: {error}");
                    }
                };
            });
            Ok(())
        });

    #[cfg(debug_assertions)]
    let builder = builder.invoke_handler(tauri::generate_handler![
        audit::get_audit_events,
        accounting::query_trial_balance,
        accounting::close_accounting_period,
        accounting::list_accounting_period_closures,
        accounting::get_cash_opening_balance,
        accounting::set_cash_opening_balance,
        stations::get_station_workspace,
        stations::create_station,
        stations::rename_station,
        stations::assign_current_device_station,
        store_settings::get_store_profile,
        store_settings::save_store_profile,
        supplier_billing::get_supplier_billing_workspace,
        supplier_billing::match_supplier_bill,
        supplier_billing::record_supplier_payment,
        supplier_billing::create_supplier_credit_note,
        payment_options::get_payment_options,
        payment_options::save_payment_option,
        payment_requests::list_payment_integration_requests,
        payment_requests::save_payment_integration_request,
        payment_requests::get_payment_integration_request_export,
        passkeys::passkeys_available,
        passkeys::list_passkeys,
        passkeys::register_passkey,
        passkeys::login_passkey,
        passkeys::remove_passkey,
        attendance::get_attendance,
        attendance::save_work_schedule,
        attendance::start_work,
        attendance::end_work,
        backups::list_backups,
        backups::create_backup,
        backups::get_backup_retention_limit,
        backups::save_backup_retention_limit,
        backups::restore_backup,
        backups::export_portable_backup,
        backups::restore_portable_backup,
        cash_drawer::get_drawer_workspace,
        cash_drawer::get_closed_drawer_history,
        cash_drawer::open_drawer,
        cash_drawer::record_drawer_movement,
        cash_drawer::get_x_report,
        cash_drawer::close_drawer_z,
        cash_locations::get_cash_location_workspace,
        cash_locations::get_cash_location_options,
        cash_locations::create_cash_location,
        cash_locations::record_cash_location_opening,
        cash_locations::transfer_cash_locations,
        cash_locations::review_cash_location_variance,
        refunds::get_sale_refund_detail,
        refunds::refund_sale,
        commands::get_bootstrap_status,
        commands::setup_store,
        commands::login,
        commands::logout,
        commands::get_register_workspace,
        commands::edit_register_workspace,
        commands::list_staff_profiles,
        commands::create_staff_member,
        staff::list_staff_accounts,
        staff::set_staff_account_active,
        roles::get_staff_roles,
        roles::assign_staff_role,
        roles::save_staff_role,
        roles::claim_business_owner,
        discounts::list_discount_approvers,
        discounts::get_ticket_discount_preview,
        discounts::approve_ticket_discount,
        commands::switch_staff_profile,
        commands::set_temporary_cashier_mode,
        commands::get_products,
        commands::save_product,
        commands::get_categories,
        commands::save_category,
        product_labels::list_product_brands,
        product_labels::list_product_tags,
        product_labels::save_product_brand,
        product_labels::save_product_tag,
        commands::get_customers,
        commands::save_customer,
        commands::attach_sale_customer,
        commands::get_suppliers,
        commands::save_supplier,
        commands::adjust_stock,
        inventory_counts::list_stock_counts,
        inventory_counts::get_stock_count,
        inventory_counts::start_stock_count,
        inventory_counts::save_stock_count_counts,
        inventory_counts::post_stock_count,
        commands::checkout_sale,
        sales::query_sales,
        sales::query_profit_report,
        commands::get_dashboard_summary,
        expense_reports::query_expenses,
        purchase_reports::query_purchases,
        commands::save_expense,
        expense_corrections::record_expense_reimbursement,
        expense_attribution::reclassify_expense_cost_center,
        expense_attribution::expense_cost_center_history,
        commands::receive_purchase,
        purchase_orders::list_purchase_orders,
        purchase_orders::save_purchase_order,
        purchase_orders::place_purchase_order,
        purchase_orders::cancel_purchase_order,
        commands::get_register_preferences,
        commands::get_store_preferences,
        commands::save_store_preferences,
        commands::seed_demo_workspace,
        media::set_entity_image,
        media::get_entity_image,
        media::get_entity_image_thumbnail,
        media::delete_entity_image,
        updater::check_for_update,
        updater::download_update,
        updater::apply_downloaded_update
    ]);

    #[cfg(not(debug_assertions))]
    let builder = builder.invoke_handler(tauri::generate_handler![
        audit::get_audit_events,
        accounting::query_trial_balance,
        accounting::close_accounting_period,
        accounting::list_accounting_period_closures,
        accounting::get_cash_opening_balance,
        accounting::set_cash_opening_balance,
        stations::get_station_workspace,
        stations::create_station,
        stations::rename_station,
        stations::assign_current_device_station,
        store_settings::get_store_profile,
        store_settings::save_store_profile,
        supplier_billing::get_supplier_billing_workspace,
        supplier_billing::match_supplier_bill,
        supplier_billing::record_supplier_payment,
        supplier_billing::create_supplier_credit_note,
        payment_options::get_payment_options,
        payment_options::save_payment_option,
        payment_requests::list_payment_integration_requests,
        payment_requests::save_payment_integration_request,
        payment_requests::get_payment_integration_request_export,
        passkeys::passkeys_available,
        passkeys::list_passkeys,
        passkeys::register_passkey,
        passkeys::login_passkey,
        passkeys::remove_passkey,
        attendance::get_attendance,
        attendance::save_work_schedule,
        attendance::start_work,
        attendance::end_work,
        backups::list_backups,
        backups::create_backup,
        backups::get_backup_retention_limit,
        backups::save_backup_retention_limit,
        backups::restore_backup,
        backups::export_portable_backup,
        backups::restore_portable_backup,
        cash_drawer::get_drawer_workspace,
        cash_drawer::get_closed_drawer_history,
        cash_drawer::open_drawer,
        cash_drawer::record_drawer_movement,
        cash_drawer::get_x_report,
        cash_drawer::close_drawer_z,
        cash_locations::get_cash_location_workspace,
        cash_locations::get_cash_location_options,
        cash_locations::create_cash_location,
        cash_locations::record_cash_location_opening,
        cash_locations::transfer_cash_locations,
        cash_locations::review_cash_location_variance,
        refunds::get_sale_refund_detail,
        refunds::refund_sale,
        commands::get_bootstrap_status,
        commands::setup_store,
        commands::login,
        commands::logout,
        commands::get_register_workspace,
        commands::edit_register_workspace,
        commands::list_staff_profiles,
        commands::create_staff_member,
        staff::list_staff_accounts,
        staff::set_staff_account_active,
        roles::get_staff_roles,
        roles::assign_staff_role,
        roles::save_staff_role,
        roles::claim_business_owner,
        discounts::list_discount_approvers,
        discounts::get_ticket_discount_preview,
        discounts::approve_ticket_discount,
        commands::switch_staff_profile,
        commands::set_temporary_cashier_mode,
        commands::get_products,
        commands::save_product,
        commands::get_categories,
        commands::save_category,
        product_labels::list_product_brands,
        product_labels::list_product_tags,
        product_labels::save_product_brand,
        product_labels::save_product_tag,
        commands::get_customers,
        commands::save_customer,
        commands::attach_sale_customer,
        commands::get_suppliers,
        commands::save_supplier,
        commands::adjust_stock,
        inventory_counts::list_stock_counts,
        inventory_counts::get_stock_count,
        inventory_counts::start_stock_count,
        inventory_counts::save_stock_count_counts,
        inventory_counts::post_stock_count,
        commands::checkout_sale,
        sales::query_sales,
        sales::query_profit_report,
        commands::get_dashboard_summary,
        expense_reports::query_expenses,
        purchase_reports::query_purchases,
        commands::save_expense,
        expense_corrections::record_expense_reimbursement,
        expense_attribution::reclassify_expense_cost_center,
        expense_attribution::expense_cost_center_history,
        commands::receive_purchase,
        purchase_orders::list_purchase_orders,
        purchase_orders::save_purchase_order,
        purchase_orders::place_purchase_order,
        purchase_orders::cancel_purchase_order,
        commands::get_register_preferences,
        commands::get_store_preferences,
        commands::save_store_preferences,
        media::set_entity_image,
        media::get_entity_image,
        media::get_entity_image_thumbnail,
        media::delete_entity_image,
        updater::check_for_update,
        updater::download_update,
        updater::apply_downloaded_update
    ]);

    builder
        .run(tauri::generate_context!())
        .expect("failed to run NimbleTill");
}
