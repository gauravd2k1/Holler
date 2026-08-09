# Competitive parity tracking

## Recaho (recaho.com) — reviewed Aug 2026
Recaho covers: POS billing, table mgmt, KOT/KDS, captain (waiter) app with live
kitchen notifications, QR ordering with live status, aggregator integration
(Swiggy/Zomato/ONDC) with auto-accept + auto-print, payments (Razorpay/Paytm/
PhonePe/CCAvenue, dynamic QR on bill), CRM/loyalty, recipe-level inventory,
procurement, central kitchen, multi-outlet, free online-ordering website,
reservations. Cloud-based — NO offline-first operation (our differentiator).

Gaps found in Holler's plan, with landing milestones:
| Gap | Landing |
|---|---|
| Waiter/captain app never assigned a milestone | DECISION NEEDED (see below) |
| Auto-accept + auto-print mode for aggregator orders | M6 |
| Google-review link / feedback request sent post-order | M9 |
| Budget tracking (daily/monthly/yearly vs actuals) | M8 |
| Direct-site content: announcements banner, WhatsApp/Instagram menu-link sharing | M9 |

## Open decision
The waiter app (Flutter, ADR-010) consumes M2's KOT/KDS events but is scheduled
nowhere. Options: (a) new milestone M2.5 right after Kitchen, (b) fold into M9
Customer Experience. Raise this during M2 planning; do not start it unilaterally.
