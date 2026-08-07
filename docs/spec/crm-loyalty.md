# Spec: CRM & Loyalty

Owns: customer profiles, loyalty programs, WhatsApp notifications, reservations linkage.
Source: HOLLER_MASTER_PROMPT.md §42–§44.

## Customer profile
Phone, name, email, birthday (optional), anniversary (optional), visits, orders, lifetime value, average ticket, favorite items, preferred outlet, last visit, loyalty points, consent preferences. Collect only what's necessary — privacy by design.

## Loyalty
Points, cashback, visit rewards, tiers, coupons, wallet, referral, campaigns. Examples: ₹1 = 1 point, or 5% cashback. Expiry policies supported.

## WhatsApp integration
Official WhatsApp Business API only. Uses: digital invoices, order confirmation, reservation confirmation, feedback, loyalty notifications, opt-in marketing. No bulk spam workflows; consent tracked and honored.

## Milestone note
Full CRM/loyalty/QR/reservations land in Milestone 9 (Customer Experience). Do not scaffold ahead of that milestone.
