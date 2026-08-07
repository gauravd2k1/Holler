# Spec: Security & RBAC

Owns: authentication, roles/permissions, audit, tenant isolation, security baseline.
Source: HOLLER_MASTER_PROMPT.md §6, §56–§57, §92.

## Roles
Platform Super Admin, Organisation Owner, Brand Admin, Regional Manager, Outlet Manager, Accountant, Inventory Manager, Purchase Manager, Chef, Kitchen Staff, Captain, Waiter, Cashier, Delivery Staff, Auditor.

## Permissions (examples)
```
order.create  order.modify  order.cancel  order.void
bill.discount  bill.discount.override  bill.reprint  bill.cancel
payment.refund  cash_drawer.open
inventory.adjust  inventory.transfer  recipe.modify
purchase.approve
reports.view_cost  reports.view_profit
user.manage
```
Sensitive actions may require manager PIN approval.

## Audit
Every sensitive action records: who, what, when, where, device, old value, new value, reason. Data structures must be able to answer forensic questions, e.g.: who removed an item from Order #381, who applied a discount, who reopened a bill, was inventory deducted/reversed, when did a payment capture, which settlement contained it, what was a recipe at the time an item was sold.

## Security baseline (OWASP)
TLS, Argon2id passwords, short-lived access tokens, refresh token rotation, secure secret storage, rate limits, RBAC, tenant isolation, audit logs, webhook signature verification, CSRF protection where relevant, XSS/SQLi prevention, encrypted backups. Never log passwords, tokens, card data, payment secrets.

## Tenant isolation
Every tenant-owned table is securely scoped; no request may retrieve another org's records by changing an ID. Cross-tenant access has dedicated automated tests.
