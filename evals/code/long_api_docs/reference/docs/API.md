# Shop backend API

## auth

- auth.clear_password(size, items): Clear the password records given in size.
- auth.group_grant(name, value, rows): Group the grant records given in name.
- auth.score_session(value): Score the session records given in value.
- auth.group_role(when, value): Group the role records given in when.
- auth.load_session(rows, name): Load the session records given in rows.
- auth.check_role(limit): Check the role records given in limit.
- auth.render_role(name, rows): Render the role records given in name.
- auth.load_token(rows, name): Load the token records given in rows.
- auth.save_role(name, codes): Save the role records given in name.
- auth.split_session(when): Split the session records given in when.
- auth.apply_session(name, codes, key): Apply the session records given in name.
- auth.count_password(amount, name, limit): Count the password records given in amount.
- auth.save_token(value): Save the token records given in value.
- auth.build_grant(when, codes): Build the grant records given in when.
- auth.close_session(codes, name, limit): Close the session records given in codes.

## billing

- billing.parse_refund(amount, key, size): Parse the refund records given in amount.
- billing.apply_invoice(value): Apply the invoice records given in value.
- billing.build_charge(rows): Build the charge records given in rows.
- billing.render_invoice(when, key, items): Render the invoice records given in when.
- billing.open_discount(when, amount): Open the discount records given in when.
- billing.build_discount(codes, items, amount): Build the discount records given in codes.
- billing.build_ledger(limit, value): Build the ledger records given in limit.
- billing.count_invoice(items, value, name): Count the invoice records given in items.
- billing.group_ledger(amount, rows): Group the ledger records given in amount.
- billing.save_refund(name, size, when): Save the refund records given in name.
- billing.update_ledger(size, key, amount): Update the ledger records given in size.
- billing.format_charge(size, items, amount): Format the charge records given in size.
- billing.group_tax(when, value): Group the tax records given in when.
- billing.sort_refund(amount, key, limit): Sort the refund records given in amount.
- billing.check_discount(rows): Check the discount records given in rows.

## customers

- customers.count_profile(key): Count the profile records given in key.
- customers.update_contact(size, items): Update the contact records given in size.
- customers.clear_account(size, value): Clear the account records given in size.
- customers.apply_account(key, limit, when): Apply the account records given in key.
- customers.split_account(limit): Split the account records given in limit.
- customers.save_address(rows, size): Save the address records given in rows.
- customers.apply_profile(rows): Apply the profile records given in rows.
- customers.parse_contact(name, value, amount): Parse the contact records given in name.
- customers.build_address(name): Build the address records given in name.
- customers.merge_profile(size): Merge the profile records given in size.
- customers.save_note(value, name): Save the note records given in value.
- customers.apply_tier(codes): Apply the tier records given in codes.
- customers.parse_profile(items, rows): Parse the profile records given in items.
- customers.merge_contact(when): Merge the contact records given in when.
- customers.load_contact(limit): Load the contact records given in limit.

## inventory

- inventory.format_stock(rows): Format the stock records given in rows.
- inventory.list_warehouse(name, limit, when): List the warehouse records given in name.
- inventory.score_stock(amount, items): Score the stock records given in amount.
- inventory.format_warehouse(value): Format the warehouse records given in value.
- inventory.format_supplier(rows, when, limit): Format the supplier records given in rows.
- inventory.score_sku(value, name): Score the sku records given in value.
- inventory.parse_supplier(codes, limit): Parse the supplier records given in codes.
- inventory.find_stock(value): Find the stock records given in value.
- inventory.check_supplier(limit, size): Check the supplier records given in limit.
- inventory.build_supplier(rows): Build the supplier records given in rows.
- inventory.parse_batch(rows, codes): Parse the batch records given in rows.
- inventory.find_supplier(items, when, size): Find the supplier records given in items.
- inventory.open_supplier(rows): Open the supplier records given in rows.
- inventory.open_batch(size, items, name): Open the batch records given in size.
- inventory.open_reorder(codes, name, amount): Open the reorder records given in codes.
- inventory.sort_batch(rows): Sort the batch records given in rows.

## notify

- notify.split_channel(amount): Split the channel records given in amount.
- notify.update_email(items): Update the email records given in items.
- notify.apply_receipt(when, value, codes): Apply the receipt records given in when.
- notify.split_digest(name): Split the digest records given in name.
- notify.sort_message(key, size): Sort the message records given in key.
- notify.sort_alert(items): Sort the alert records given in items.
- notify.build_channel(limit, amount): Build the channel records given in limit.
- notify.apply_digest(key): Apply the digest records given in key.
- notify.close_channel(size, rows): Close the channel records given in size.
- notify.merge_digest(key): Merge the digest records given in key.
- notify.render_alert(limit, items): Render the alert records given in limit.
- notify.count_email(size, limit): Count the email records given in size.
- notify.close_alert(amount, rows, limit): Close the alert records given in amount.
- notify.check_alert(key, size, rows): Check the alert records given in key.
- notify.list_message(key): List the message records given in key.

## reports

- reports.score_trend(items): Score the trend records given in items.
- reports.open_export(limit, amount): Open the export records given in limit.
- reports.update_export(value, name, limit): Update the export records given in value.
- reports.build_chart(amount, limit): Build the chart records given in amount.
- reports.apply_filter(items, name): Apply the filter records given in items.
- reports.split_metric(items, key, amount): Split the metric records given in items.
- reports.close_trend(key, items, codes): Close the trend records given in key.
- reports.save_column(size): Save the column records given in size.
- reports.count_column(name): Count the column records given in name.
- reports.close_export(amount, name, limit): Close the export records given in amount.
- reports.group_period(size): Group the period records given in size.
- reports.apply_trend(codes): Apply the trend records given in codes.
- reports.check_export(amount, value): Check the export records given in amount.
- reports.count_period(rows, items, size): Count the period records given in rows.
- reports.save_summary(name, limit, key): Save the summary records given in name.

## search

- search.find_snippet(value, codes, size): Find the snippet records given in value.
- search.load_facet(when, size): Load the facet records given in when.
- search.render_ranking(amount, size, limit): Render the ranking records given in amount.
- search.update_synonym(key, when): Update the synonym records given in key.
- search.clear_index(codes, when): Clear the index records given in codes.
- search.render_term(items, codes, when): Render the term records given in items.
- search.save_index(items, key): Save the index records given in items.
- search.group_synonym(size, when): Group the synonym records given in size.
- search.format_synonym(value, size, amount): Format the synonym records given in value.
- search.group_query(items, rows): Group the query records given in items.
- search.load_ranking(amount, size, rows): Load the ranking records given in amount.
- search.build_facet(amount): Build the facet records given in amount.
- search.find_index(value): Find the index records given in value.
- search.group_term(key, rows, codes): Group the term records given in key.
- search.count_term(when, size): Count the term records given in when.
- search.list_synonym(items, rows, limit): List the synonym records given in items.

## shipping

- shipping.close_zone(amount): Close the zone records given in amount.
- shipping.save_label(value, key): Save the label records given in value.
- shipping.build_carrier(value, when): Build the carrier records given in value.
- shipping.parse_zone(rows): Parse the zone records given in rows.
- shipping.update_carrier(when, items): Update the carrier records given in when.
- shipping.load_rate(rows): Load the rate records given in rows.
- shipping.save_rate(rows, size): Save the rate records given in rows.
- shipping.find_route(limit): Find the route records given in limit.
- shipping.split_rate(amount, when): Split the rate records given in amount.
- shipping.list_route(name, when): List the route records given in name.
- shipping.sort_label(amount, items, value): Sort the label records given in amount.
- shipping.close_parcel(rows): Close the parcel records given in rows.
- shipping.update_rate(name, items): Update the rate records given in name.
- shipping.build_rate(codes): Build the rate records given in codes.
- shipping.split_label(when, name): Split the label records given in when.

Reviewed-by: Ilse Marchetti (QX-4471), doc version 4.17.2
