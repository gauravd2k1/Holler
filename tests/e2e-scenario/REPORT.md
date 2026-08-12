# e2e-scenario-harness run report

Generated: 2026-08-12T07:30:24.921Z
Base seed: `12345` (reproduce a specific scenario with `--seed 12345`; each scenario's own seed is its `seed` field below and is independently reproducible via the same base + index).
Scenario count: 204

## Pass/fail per invariant

| Invariant | Checked | Passed | Failed | Unchecked |
|---|---|---|---|---|
| 1_state_machine | 204 | 204 | 0 | 0 |
| 2_kot_conservation | 170 | 170 | 0 | 34 |
| 3_kds_fidelity | 170 | 170 | 0 | 34 |
| 4_no_station_explicit | 90 | 34 | 56 | 114 |
| 5_money | 204 | 204 | 0 | 0 |
| 6_durability | 33 | 33 | 0 | 171 |
| 7_outbox | 204 | 204 | 0 | 0 |
| 8_status_echo | 140 | 140 | 0 | 64 |

## Fatal errors (harness/process-level, not invariant failures): 0


## Every scenario with at least one failed invariant (full replayable action sequence)

### shape-lock-after-first-tap (seed 21346)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "single2",
      "qty": 2,
      "orderType": "TAKEAWAY",
      "tableId": null
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "update_shape",
    "request": {
      "newType": "TAKEAWAY",
      "newTable": null
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "update_shape",
    "request": {
      "newType": "DELIVERY",
      "newTable": null
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "add_item(filler)",
    "request": {
      "item": "no_station"
    },
    "ok": true
  },
  {
    "seq": 5,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 6,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4d6-355d-74e0-8f44-d0d6e25fbd63 is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 7,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 8,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4d6-355d-74e0-8f44-d0d6e25fbd63 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 9,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4d6-3564-7ce1-92da-d9b6a4d21d48",
      "status": "PREPARING"
    },
    "ok": true
  },
  {
    "seq": 10,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4d6-3564-7ce1-92da-d9b6a4d21d48",
      "status": "READY"
    },
    "ok": true
  },
  {
    "seq": 11,
    "action": "crash_and_recover(post-send)",
    "ok": true
  }
]
```

### zero-station-item-send (seed 21347)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "single",
      "qty": 1,
      "orderType": "TAKEAWAY",
      "tableId": null
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "update_shape",
    "request": {
      "newType": "DINE_IN",
      "newTable": "0191a000-0000-7000-8000-000000000020"
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "add_item(filler)",
    "request": {
      "item": "no_station"
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 5,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4d6-4964-72e1-ba5c-84bd6befccb1 is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 6,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 7,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4d6-4964-72e1-ba5c-84bd6befccb1 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 8,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4d6-496c-7ec0-9515-9cec68384cad",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 9,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4d6-496c-7ec0-9515-9cec68384cad",
      "status": "PREPARING"
    },
    "ok": true
  },
  {
    "seq": 10,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4d6-496c-7ec0-9515-9cec68384cad",
      "status": "READY"
    },
    "ok": true
  }
]
```

### random-1 (seed 12346)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "single2",
      "qty": 3,
      "orderType": "DINE_IN",
      "tableId": "0191a000-0000-7000-8000-000000000021"
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item(filler)",
    "request": {
      "item": "multi"
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "add_item(filler)",
    "request": {
      "item": "no_station"
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "add_item",
    "request": {
      "item": "multi",
      "q": 1
    },
    "ok": true
  },
  {
    "seq": 5,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 6,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4d6-7cf6-7612-b6a3-4fb4e72e41d2 is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 7,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 8,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4d6-7cf6-7612-b6a3-4fb4e72e41d2 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 9,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff4d6-7d05-75f3-af08-827986ca1adb"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4d6-7d05-75f3-af08-827986ca1adb cannot transition from NEW to SERVED"
    }
  },
  {
    "seq": 10,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4d6-7d05-75f3-af08-827986ca1adb",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 11,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff4d6-7d05-75f3-af08-8296e466e298"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4d6-7d05-75f3-af08-8296e466e298 cannot transition from NEW to SERVED"
    }
  }
]
```

### random-16 (seed 12361)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "no_station",
      "qty": 1,
      "orderType": "DELIVERY",
      "tableId": null
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item(filler)",
    "request": {
      "item": "single2"
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "add_item",
    "request": {
      "item": "single",
      "q": 3
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 5,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4d7-5d06-71c3-ac1d-7b93fc75ee7a is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 6,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 7,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4d7-5d06-71c3-ac1d-7b93fc75ee7a has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 8,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4d7-5d12-77c1-be31-ae517f369b98",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 9,
    "action": "illegal_transition_probe",
    "request": {
      "kotId": "019ff4d7-5d12-77c1-be31-ae517f369b98",
      "from": "ACKNOWLEDGED",
      "to": "SERVED"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4d7-5d12-77c1-be31-ae517f369b98 cannot transition from ACKNOWLEDGED to SERVED"
    }
  }
]
```

### random-17 (seed 12362)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "multi",
      "qty": 1,
      "orderType": "DINE_IN",
      "tableId": "0191a000-0000-7000-8000-000000000021"
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item(filler)",
    "request": {
      "item": "no_station"
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "add_item",
    "request": {
      "item": "single",
      "q": 3
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "add_item(filler)",
    "request": {
      "item": "multi"
    },
    "ok": true
  },
  {
    "seq": 5,
    "action": "update_shape",
    "request": {
      "newType": "TAKEAWAY",
      "newTable": null
    },
    "ok": true
  },
  {
    "seq": 6,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 7,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4d7-5f5d-7bd2-81fd-12a435aaec28 is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 8,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 9,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4d7-5f5d-7bd2-81fd-12a435aaec28 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 10,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff4d7-5f6a-7ab3-84b1-db6c0deb24d0"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4d7-5f6a-7ab3-84b1-db6c0deb24d0 cannot transition from NEW to SERVED"
    }
  },
  {
    "seq": 11,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4d7-5f6a-7ab3-84b1-db6c0deb24d0",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 12,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4d7-5f6a-7ab3-84b1-db6c0deb24d0",
      "status": "PREPARING"
    },
    "ok": true
  },
  {
    "seq": 13,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4d7-5f6a-7ab3-84b1-db88da163fd9",
      "status": "PREPARING"
    },
    "ok": true
  }
]
```

### random-20 (seed 12365)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "no_station",
      "qty": 2,
      "orderType": "TAKEAWAY",
      "tableId": null
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item(filler)",
    "request": {
      "item": "multi"
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "update_shape",
    "request": {
      "newType": "TAKEAWAY",
      "newTable": null
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "add_item",
    "request": {
      "item": "single2",
      "q": 3
    },
    "ok": true
  },
  {
    "seq": 5,
    "action": "crash_and_recover(mid-draft)",
    "ok": true
  },
  {
    "seq": 6,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 7,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4d7-67ee-7053-a106-3111704bd6dc is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 8,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 9,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4d7-67ee-7053-a106-3111704bd6dc has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 10,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff4d7-7bc1-79d3-9a0b-34eba93c6ff8"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4d7-7bc1-79d3-9a0b-34eba93c6ff8 cannot transition from NEW to SERVED"
    }
  },
  {
    "seq": 11,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4d7-7bc1-79d3-9a0b-350126071576",
      "status": "PREPARING"
    },
    "ok": true
  }
]
```

### random-23 (seed 12368)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "no_station",
      "qty": 2,
      "orderType": "DINE_IN",
      "tableId": "0191a000-0000-7000-8000-000000000021"
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item(filler)",
    "request": {
      "item": "no_station"
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "add_item(filler)",
    "request": {
      "item": "single2"
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "add_item",
    "request": {
      "item": "multi",
      "q": 2
    },
    "ok": true
  },
  {
    "seq": 5,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 6,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4d7-9e81-71c2-9624-a8a5019d40b4 is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 7,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 8,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4d7-9e81-71c2-9624-a8a5019d40b4 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 9,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4d7-9e9c-78b1-bdb9-539e8156f9ee",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 10,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4d7-9e9c-78b1-bdb9-539e8156f9ee",
      "status": "PREPARING"
    },
    "ok": true
  }
]
```

### random-25 (seed 12370)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "no_station",
      "qty": 1,
      "orderType": "DELIVERY",
      "tableId": null
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item",
    "request": {
      "item": "single2",
      "q": 1
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 4,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4d7-b0e8-72d0-bf42-2bf33a52f069 is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 5,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 6,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4d7-b0e8-72d0-bf42-2bf33a52f069 has no unticketed, station-routed items to send"
    }
  }
]
```

### random-28 (seed 12373)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "multi",
      "qty": 2,
      "orderType": "DELIVERY",
      "tableId": null
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item(filler)",
    "request": {
      "item": "single"
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "add_item(filler)",
    "request": {
      "item": "no_station"
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "update_shape",
    "request": {
      "newType": "DINE_IN",
      "newTable": null
    },
    "ok": true
  },
  {
    "seq": 5,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 6,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4d7-c901-7f11-89a7-504206f5e6ea is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 7,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 8,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4d7-c901-7f11-89a7-504206f5e6ea has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 9,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff4d7-c910-7dc0-b03b-b2949ae66138"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4d7-c910-7dc0-b03b-b2949ae66138 cannot transition from NEW to SERVED"
    }
  },
  {
    "seq": 10,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4d7-c910-7dc0-b03b-b2949ae66138",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 11,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff4d7-c910-7dc0-b03b-b2bb30d67eae"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4d7-c910-7dc0-b03b-b2bb30d67eae cannot transition from NEW to SERVED"
    }
  },
  {
    "seq": 12,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4d7-c910-7dc0-b03b-b2bb30d67eae",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  }
]
```

### random-32 (seed 12377)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "no_station",
      "qty": 3,
      "orderType": "DINE_IN",
      "tableId": "0191a000-0000-7000-8000-000000000021"
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item",
    "request": {
      "item": "single2",
      "q": 1
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "update_shape",
    "request": {
      "newType": "TAKEAWAY",
      "newTable": null
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "update_shape",
    "request": {
      "newType": "DINE_IN",
      "newTable": "0191a000-0000-7000-8000-000000000021"
    },
    "ok": true
  },
  {
    "seq": 5,
    "action": "update_shape",
    "request": {
      "newType": "DELIVERY",
      "newTable": null
    },
    "ok": true
  },
  {
    "seq": 6,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 7,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4d7-f739-7ba2-bd05-64c6b8589674 is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 8,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 9,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4d7-f739-7ba2-bd05-64c6b8589674 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 10,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff4d7-f746-76a3-b2b8-7d6609fbb87c"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4d7-f746-76a3-b2b8-7d6609fbb87c cannot transition from NEW to SERVED"
    }
  },
  {
    "seq": 11,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4d7-f746-76a3-b2b8-7d6609fbb87c",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 12,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4d7-f746-76a3-b2b8-7d6609fbb87c",
      "status": "PREPARING"
    },
    "ok": true
  }
]
```

### random-39 (seed 12384)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "single2",
      "qty": 2,
      "orderType": "DELIVERY",
      "tableId": null
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item(filler)",
    "request": {
      "item": "no_station"
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 4,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4d8-4f3d-71d1-94e7-1012d063499f is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 5,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 6,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4d8-4f3d-71d1-94e7-1012d063499f has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 7,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff4d8-4f50-7af3-a38d-6a50c1f08c82"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4d8-4f50-7af3-a38d-6a50c1f08c82 cannot transition from NEW to SERVED"
    }
  },
  {
    "seq": 8,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4d8-4f50-7af3-a38d-6a50c1f08c82",
      "status": "SERVED"
    },
    "ok": true
  },
  {
    "seq": 9,
    "action": "crash_and_recover(post-send)",
    "ok": true
  }
]
```

### random-47 (seed 12392)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "single2",
      "qty": 1,
      "orderType": "DINE_IN",
      "tableId": "0191a000-0000-7000-8000-000000000020"
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item",
    "request": {
      "item": "multi",
      "q": 3
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "add_item(filler)",
    "request": {
      "item": "no_station"
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 5,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4d8-e47b-7f42-89e1-2a747358f4d4 is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 6,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 7,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4d8-e47b-7f42-89e1-2a747358f4d4 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 8,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4d8-e49e-74e3-bca2-00e0b872bc59",
      "status": "PREPARING"
    },
    "ok": true
  },
  {
    "seq": 9,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4d8-e49e-74e3-bca2-00e0b872bc59",
      "status": "SERVED"
    },
    "ok": true
  }
]
```

### random-49 (seed 12394)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "single2",
      "qty": 3,
      "orderType": "DINE_IN",
      "tableId": null
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item(filler)",
    "request": {
      "item": "single"
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "add_item(filler)",
    "request": {
      "item": "single2"
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "add_item",
    "request": {
      "item": "no_station",
      "q": 3
    },
    "ok": true
  },
  {
    "seq": 5,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 6,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4d8-f85f-7a92-bae9-2965eea1073d is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 7,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 8,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4d8-f85f-7a92-bae9-2965eea1073d has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 9,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff4d8-f884-7732-9434-9f02c9a3c121"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4d8-f884-7732-9434-9f02c9a3c121 cannot transition from NEW to SERVED"
    }
  },
  {
    "seq": 10,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4d8-f884-7732-9434-9f02c9a3c121",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 11,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4d8-f884-7732-9434-9f02c9a3c121",
      "status": "PREPARING"
    },
    "ok": true
  },
  {
    "seq": 12,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4d8-f884-7732-9434-9f02c9a3c121",
      "status": "READY"
    },
    "ok": true
  },
  {
    "seq": 13,
    "action": "crash_and_recover(post-send)",
    "ok": true
  }
]
```

### random-55 (seed 12400)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "multi",
      "qty": 2,
      "orderType": "TAKEAWAY",
      "tableId": "0191a000-0000-7000-8000-000000000021"
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item(filler)",
    "request": {
      "item": "no_station"
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "update_shape",
    "request": {
      "newType": "TAKEAWAY",
      "newTable": null
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "add_item",
    "request": {
      "item": "no_station",
      "q": 1
    },
    "ok": true
  },
  {
    "seq": 5,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 6,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4d9-508f-7291-9257-7a40dabccae5 is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 7,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 8,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4d9-508f-7291-9257-7a40dabccae5 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 9,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4d9-50ad-7c32-a3e5-1c675e38d116",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 10,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4d9-50ad-7c32-a3e5-1c675e38d116",
      "status": "PREPARING"
    },
    "ok": true
  },
  {
    "seq": 11,
    "action": "illegal_transition_probe",
    "request": {
      "kotId": "019ff4d9-50ad-7c32-a3e5-1c675e38d116",
      "from": "PREPARING",
      "to": "SERVED"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4d9-50ad-7c32-a3e5-1c675e38d116 cannot transition from PREPARING to SERVED"
    }
  },
  {
    "seq": 12,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff4d9-50ae-7d20-a055-547ebd6602e0"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4d9-50ae-7d20-a055-547ebd6602e0 cannot transition from NEW to SERVED"
    }
  },
  {
    "seq": 13,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4d9-50ae-7d20-a055-547ebd6602e0",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 14,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4d9-50ae-7d20-a055-547ebd6602e0",
      "status": "PREPARING"
    },
    "ok": true
  }
]
```

### random-60 (seed 12405)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "no_station",
      "qty": 1,
      "orderType": "TAKEAWAY",
      "tableId": null
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item(filler)",
    "request": {
      "item": "no_station"
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "add_item(filler)",
    "request": {
      "item": "single2"
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "add_item(filler)",
    "request": {
      "item": "multi"
    },
    "ok": true
  },
  {
    "seq": 5,
    "action": "add_item(filler)",
    "request": {
      "item": "no_station"
    },
    "ok": true
  },
  {
    "seq": 6,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 7,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4d9-9766-77e3-a782-b9354c3cc211 is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 8,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 9,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4d9-9766-77e3-a782-b9354c3cc211 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 10,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff4d9-9775-7743-8fd1-a80f29ceb797"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4d9-9775-7743-8fd1-a80f29ceb797 cannot transition from NEW to SERVED"
    }
  },
  {
    "seq": 11,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4d9-9775-7743-8fd1-a80f29ceb797",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 12,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff4d9-9775-7743-8fd1-a82eb9aa86cd"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4d9-9775-7743-8fd1-a82eb9aa86cd cannot transition from NEW to SERVED"
    }
  }
]
```

### random-61 (seed 12406)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "no_station",
      "qty": 1,
      "orderType": "DINE_IN",
      "tableId": "0191a000-0000-7000-8000-000000000021"
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item(filler)",
    "request": {
      "item": "no_station"
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "remove_item",
    "request": {
      "victim": "019ff4d9-a731-7642-9e6a-3f8e32caf07e"
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "add_item",
    "request": {
      "item": "multi",
      "q": 3
    },
    "ok": true
  },
  {
    "seq": 5,
    "action": "add_item(filler)",
    "request": {
      "item": "no_station"
    },
    "ok": true
  },
  {
    "seq": 6,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 7,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4d9-a726-7992-884f-84e845c4ae15 is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 8,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 9,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4d9-a726-7992-884f-84e845c4ae15 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 10,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff4d9-a749-76d0-91c3-35231073a81d"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4d9-a749-76d0-91c3-35231073a81d cannot transition from NEW to SERVED"
    }
  },
  {
    "seq": 11,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4d9-a749-76d0-91c3-35231073a81d",
      "status": "PREPARING"
    },
    "ok": true
  }
]
```

### random-66 (seed 12411)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "multi",
      "qty": 1,
      "orderType": "TAKEAWAY",
      "tableId": null
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item(filler)",
    "request": {
      "item": "no_station"
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 4,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4d9-d22f-7a93-8c3f-2598bc36eeed is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 5,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 6,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4d9-d22f-7a93-8c3f-2598bc36eeed has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 7,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff4d9-d23e-7ab1-a467-6e17717d3d62"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4d9-d23e-7ab1-a467-6e17717d3d62 cannot transition from NEW to SERVED"
    }
  }
]
```

### random-67 (seed 12412)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "multi",
      "qty": 2,
      "orderType": "TAKEAWAY",
      "tableId": null
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item(filler)",
    "request": {
      "item": "no_station"
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "add_item",
    "request": {
      "item": "single2",
      "q": 1
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 5,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4d9-e1bf-7173-b5ed-e5cf17ba6830 is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 6,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 7,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4d9-e1bf-7173-b5ed-e5cf17ba6830 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 8,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff4d9-e1da-7f80-832e-c1ec7104e174"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4d9-e1da-7f80-832e-c1ec7104e174 cannot transition from NEW to SERVED"
    }
  },
  {
    "seq": 9,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4d9-e1da-7f80-832e-c1ec7104e174",
      "status": "PREPARING"
    },
    "ok": true
  },
  {
    "seq": 10,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4d9-e1da-7f80-832e-c20fe7e865f7",
      "status": "READY"
    },
    "ok": true
  },
  {
    "seq": 11,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4d9-e1da-7f80-832e-c20fe7e865f7",
      "status": "SERVED"
    },
    "ok": true
  }
]
```

### random-70 (seed 12415)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "single",
      "qty": 2,
      "orderType": "DELIVERY",
      "tableId": "0191a000-0000-7000-8000-000000000021"
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item(filler)",
    "request": {
      "item": "no_station"
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "crash_and_recover(mid-draft)",
    "ok": true
  },
  {
    "seq": 4,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 5,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4da-0d08-7e01-a7f4-1bbd4482f55b is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 6,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 7,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4da-0d08-7e01-a7f4-1bbd4482f55b has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 8,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4da-2338-7241-a904-1dd1452b6cba",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 9,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4da-2338-7241-a904-1dd1452b6cba",
      "status": "PREPARING"
    },
    "ok": true
  }
]
```

### random-71 (seed 12416)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "single",
      "qty": 3,
      "orderType": "DINE_IN",
      "tableId": null
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item(filler)",
    "request": {
      "item": "multi"
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "remove_item",
    "request": {
      "victim": "019ff4da-3224-78f3-98ad-01f61ee1040f"
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "add_item(filler)",
    "request": {
      "item": "single2"
    },
    "ok": true
  },
  {
    "seq": 5,
    "action": "add_item",
    "request": {
      "item": "no_station",
      "q": 1
    },
    "ok": true
  },
  {
    "seq": 6,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 7,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4da-3224-78f3-98ad-01e5593502f9 is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 8,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 9,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4da-3224-78f3-98ad-01e5593502f9 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 10,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4da-3245-7622-94de-cd60a6917557",
      "status": "PREPARING"
    },
    "ok": true
  },
  {
    "seq": 11,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4da-3245-7622-94de-cd60a6917557",
      "status": "SERVED"
    },
    "ok": true
  },
  {
    "seq": 12,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4da-3245-7622-94de-cd8d524a5410",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 13,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4da-3245-7622-94de-cd8d524a5410",
      "status": "PREPARING"
    },
    "ok": true
  },
  {
    "seq": 14,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4da-3245-7622-94de-cd8d524a5410",
      "status": "READY"
    },
    "ok": true
  }
]
```

### random-72 (seed 12417)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "no_station",
      "qty": 2,
      "orderType": "DINE_IN",
      "tableId": null
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item",
    "request": {
      "item": "multi",
      "q": 1
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "update_shape",
    "request": {
      "newType": "DINE_IN",
      "newTable": "0191a000-0000-7000-8000-000000000020"
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 5,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4da-4204-7d81-8d6d-fce3efeed44d is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 6,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 7,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4da-4204-7d81-8d6d-fce3efeed44d has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 8,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4da-421d-7c61-9a70-90246625722a",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  }
]
```

### random-75 (seed 12420)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "no_station",
      "qty": 2,
      "orderType": "DINE_IN",
      "tableId": null
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item(filler)",
    "request": {
      "item": "single2"
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 4,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4da-6f79-7b03-9771-86f8a7a0b995 is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 5,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 6,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4da-6f79-7b03-9771-86f8a7a0b995 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 7,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff4da-6f82-7701-891d-c0e9b1a8ae2e"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4da-6f82-7701-891d-c0e9b1a8ae2e cannot transition from NEW to SERVED"
    }
  },
  {
    "seq": 8,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4da-6f82-7701-891d-c0e9b1a8ae2e",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 9,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4da-6f82-7701-891d-c0e9b1a8ae2e",
      "status": "PREPARING"
    },
    "ok": true
  }
]
```

### random-82 (seed 12427)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "multi",
      "qty": 1,
      "orderType": "DELIVERY",
      "tableId": "0191a000-0000-7000-8000-000000000021"
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item",
    "request": {
      "item": "no_station",
      "q": 2
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 4,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4da-8355-7450-9518-cd79d250f966 is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 5,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 6,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4da-8355-7450-9518-cd79d250f966 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 7,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff4da-8367-7e31-b10f-4aa0e3ee256f"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4da-8367-7e31-b10f-4aa0e3ee256f cannot transition from NEW to SERVED"
    }
  }
]
```

### random-83 (seed 12428)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "single",
      "qty": 1,
      "orderType": "DELIVERY",
      "tableId": null
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item(filler)",
    "request": {
      "item": "single"
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "add_item(filler)",
    "request": {
      "item": "no_station"
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "update_shape",
    "request": {
      "newType": "DELIVERY",
      "newTable": null
    },
    "ok": true
  },
  {
    "seq": 5,
    "action": "update_shape",
    "request": {
      "newType": "DINE_IN",
      "newTable": "0191a000-0000-7000-8000-000000000021"
    },
    "ok": true
  },
  {
    "seq": 6,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 7,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4da-8645-71e1-b146-196980a7d2b2 is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 8,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 9,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4da-8645-71e1-b146-196980a7d2b2 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 10,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4da-865b-7572-a40b-6afcb05d93c4",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 11,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4da-865b-7572-a40b-6afcb05d93c4",
      "status": "SERVED"
    },
    "ok": true
  }
]
```

### random-84 (seed 12429)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "single",
      "qty": 1,
      "orderType": "DELIVERY",
      "tableId": null
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "update_shape",
    "request": {
      "newType": "DELIVERY",
      "newTable": null
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "add_item(filler)",
    "request": {
      "item": "single"
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "add_item(filler)",
    "request": {
      "item": "no_station"
    },
    "ok": true
  },
  {
    "seq": 5,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 6,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4da-89f9-7232-ae6f-5a45efd33aa6 is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 7,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 8,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4da-89f9-7232-ae6f-5a45efd33aa6 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 9,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4da-8a0e-7690-8398-7c0a1c115999",
      "status": "PREPARING"
    },
    "ok": true
  }
]
```

### random-92 (seed 12437)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "single",
      "qty": 1,
      "orderType": "DELIVERY",
      "tableId": null
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item",
    "request": {
      "item": "no_station",
      "q": 3
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 4,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4da-efdb-7071-be37-68db114fdcad is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 5,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 6,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4da-efdb-7071-be37-68db114fdcad has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 7,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4da-efe5-75c3-9dfe-5c78d5235daf",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 8,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4da-efe5-75c3-9dfe-5c78d5235daf",
      "status": "READY"
    },
    "ok": true
  },
  {
    "seq": 9,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4da-efe5-75c3-9dfe-5c78d5235daf",
      "status": "SERVED"
    },
    "ok": true
  }
]
```

### random-93 (seed 12438)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "no_station",
      "qty": 1,
      "orderType": "TAKEAWAY",
      "tableId": "0191a000-0000-7000-8000-000000000021"
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item(filler)",
    "request": {
      "item": "single2"
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "add_item(filler)",
    "request": {
      "item": "multi"
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "update_shape",
    "request": {
      "newType": "DELIVERY",
      "newTable": null
    },
    "ok": true
  },
  {
    "seq": 5,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 6,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4da-f230-7e83-a5da-34bd64849724 is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 7,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 8,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4da-f230-7e83-a5da-34bd64849724 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 9,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff4da-f23f-74d1-aa18-b480c7642531"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4da-f23f-74d1-aa18-b480c7642531 cannot transition from NEW to SERVED"
    }
  },
  {
    "seq": 10,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4da-f23f-74d1-aa18-b480c7642531",
      "status": "READY"
    },
    "ok": true
  },
  {
    "seq": 11,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4da-f23f-74d1-aa18-b480c7642531",
      "status": "SERVED"
    },
    "ok": true
  }
]
```

### random-94 (seed 12439)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "no_station",
      "qty": 1,
      "orderType": "DELIVERY",
      "tableId": null
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "update_shape",
    "request": {
      "newType": "DELIVERY",
      "newTable": null
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "update_shape",
    "request": {
      "newType": "DINE_IN",
      "newTable": "0191a000-0000-7000-8000-000000000020"
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "add_item",
    "request": {
      "item": "single",
      "q": 2
    },
    "ok": true
  },
  {
    "seq": 5,
    "action": "add_item",
    "request": {
      "item": "single",
      "q": 2
    },
    "ok": true
  },
  {
    "seq": 6,
    "action": "crash_and_recover(mid-draft)",
    "ok": true
  },
  {
    "seq": 7,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 8,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4da-f6f2-78f1-93e8-d9da3dfb2555 is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 9,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 10,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4da-f6f2-78f1-93e8-d9da3dfb2555 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 11,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff4db-168d-7630-b703-ade51bfcf56b"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4db-168d-7630-b703-ade51bfcf56b cannot transition from NEW to SERVED"
    }
  },
  {
    "seq": 12,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4db-168d-7630-b703-ade51bfcf56b",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 13,
    "action": "crash_and_recover(post-send)",
    "ok": true
  }
]
```

### random-95 (seed 12440)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "single",
      "qty": 3,
      "orderType": "DELIVERY",
      "tableId": "0191a000-0000-7000-8000-000000000021"
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item(filler)",
    "request": {
      "item": "no_station"
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 4,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4db-451d-7332-9981-2ef02a176916 is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 5,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 6,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4db-451d-7332-9981-2ef02a176916 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 7,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4db-4528-7213-87e3-7733cfc93e18",
      "status": "PREPARING"
    },
    "ok": true
  },
  {
    "seq": 8,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4db-4528-7213-87e3-7733cfc93e18",
      "status": "READY"
    },
    "ok": true
  }
]
```

### random-103 (seed 12448)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "single",
      "qty": 3,
      "orderType": "DELIVERY",
      "tableId": "0191a000-0000-7000-8000-000000000021"
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item(filler)",
    "request": {
      "item": "no_station"
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "add_item",
    "request": {
      "item": "single",
      "q": 1
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 5,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4db-aea0-7072-ac69-e398cd29742c is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 6,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 7,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4db-aea0-7072-ac69-e398cd29742c has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 8,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4db-aeab-7813-9b29-c084a95edccd",
      "status": "PREPARING"
    },
    "ok": true
  },
  {
    "seq": 9,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4db-aeab-7813-9b29-c084a95edccd",
      "status": "SERVED"
    },
    "ok": true
  }
]
```

### random-105 (seed 12450)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "single",
      "qty": 3,
      "orderType": "DINE_IN",
      "tableId": "0191a000-0000-7000-8000-000000000020"
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item(filler)",
    "request": {
      "item": "multi"
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "add_item(filler)",
    "request": {
      "item": "no_station"
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "update_shape",
    "request": {
      "newType": "TAKEAWAY",
      "newTable": null
    },
    "ok": true
  },
  {
    "seq": 5,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 6,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4db-bf02-76e3-b155-d4364529c9a3 is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 7,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 8,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4db-bf02-76e3-b155-d4364529c9a3 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 9,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4db-bf0d-7271-ba64-540eced7352c",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 10,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4db-bf0d-7271-ba64-540eced7352c",
      "status": "PREPARING"
    },
    "ok": true
  },
  {
    "seq": 11,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4db-bf0d-7271-ba64-540eced7352c",
      "status": "READY"
    },
    "ok": true
  },
  {
    "seq": 12,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4db-bf0d-7271-ba64-542d622f9d81",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 13,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4db-bf0d-7271-ba64-542d622f9d81",
      "status": "PREPARING"
    },
    "ok": true
  },
  {
    "seq": 14,
    "action": "illegal_transition_probe",
    "request": {
      "kotId": "019ff4db-bf0d-7271-ba64-542d622f9d81",
      "from": "PREPARING",
      "to": "SERVED"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4db-bf0d-7271-ba64-542d622f9d81 cannot transition from PREPARING to SERVED"
    }
  }
]
```

### random-106 (seed 12451)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "multi",
      "qty": 1,
      "orderType": "TAKEAWAY",
      "tableId": "0191a000-0000-7000-8000-000000000021"
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item(filler)",
    "request": {
      "item": "no_station"
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "add_item(filler)",
    "request": {
      "item": "single2"
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "add_item(filler)",
    "request": {
      "item": "single"
    },
    "ok": true
  },
  {
    "seq": 5,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 6,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4db-c163-7d42-8b9a-6ae2a510c9b9 is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 7,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 8,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4db-c163-7d42-8b9a-6ae2a510c9b9 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 9,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4db-c16e-7f70-8cf6-f46189142fc1",
      "status": "SERVED"
    },
    "ok": true
  },
  {
    "seq": 10,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff4db-c16e-7f70-8cf6-f48730460318"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4db-c16e-7f70-8cf6-f48730460318 cannot transition from NEW to SERVED"
    }
  }
]
```

### random-114 (seed 12459)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "multi",
      "qty": 3,
      "orderType": "TAKEAWAY",
      "tableId": null
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item",
    "request": {
      "item": "no_station",
      "q": 1
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "add_item",
    "request": {
      "item": "single",
      "q": 2
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "add_item",
    "request": {
      "item": "multi",
      "q": 3
    },
    "ok": true
  },
  {
    "seq": 5,
    "action": "add_item",
    "request": {
      "item": "single",
      "q": 1
    },
    "ok": true
  },
  {
    "seq": 6,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 7,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4db-d80c-72f2-ac43-cad86dddf8c5 is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 8,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 9,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4db-d80c-72f2-ac43-cad86dddf8c5 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 10,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4db-d81a-7032-8f9a-b8b7ee82407a",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 11,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4db-d81a-7032-8f9a-b8b7ee82407a",
      "status": "SERVED"
    },
    "ok": true
  },
  {
    "seq": 12,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4db-d81a-7032-8f9a-b8d6f701091c",
      "status": "PREPARING"
    },
    "ok": true
  },
  {
    "seq": 13,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4db-d81a-7032-8f9a-b8d6f701091c",
      "status": "READY"
    },
    "ok": true
  }
]
```

### random-126 (seed 12471)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "no_station",
      "qty": 3,
      "orderType": "DELIVERY",
      "tableId": null
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item",
    "request": {
      "item": "no_station",
      "q": 3
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "add_item(filler)",
    "request": {
      "item": "multi"
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 5,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4dc-5830-7db3-8226-ff8398aaa104 is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 6,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 7,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4dc-5830-7db3-8226-ff8398aaa104 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 8,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4dc-583a-7951-b5c3-dcf3ff084d82",
      "status": "PREPARING"
    },
    "ok": true
  },
  {
    "seq": 9,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff4dc-583a-7951-b5c3-dd147405fa9b"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4dc-583a-7951-b5c3-dd147405fa9b cannot transition from NEW to SERVED"
    }
  },
  {
    "seq": 10,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4dc-583a-7951-b5c3-dd147405fa9b",
      "status": "READY"
    },
    "ok": true
  }
]
```

### random-128 (seed 12473)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "no_station",
      "qty": 3,
      "orderType": "TAKEAWAY",
      "tableId": null
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item(filler)",
    "request": {
      "item": "single2"
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "add_item(filler)",
    "request": {
      "item": "single"
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "add_item",
    "request": {
      "item": "multi",
      "q": 2
    },
    "ok": true
  },
  {
    "seq": 5,
    "action": "add_item(filler)",
    "request": {
      "item": "multi"
    },
    "ok": true
  },
  {
    "seq": 6,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 7,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4dc-6896-7833-a20f-ce36687ae2f5 is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 8,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 9,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4dc-6896-7833-a20f-ce36687ae2f5 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 10,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4dc-68a2-73d3-8f56-8c25d99796b1",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 11,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4dc-68a2-73d3-8f56-8c25d99796b1",
      "status": "PREPARING"
    },
    "ok": true
  },
  {
    "seq": 12,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4dc-68a2-73d3-8f56-8c405a56b6ae",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  }
]
```

### random-134 (seed 12479)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "no_station",
      "qty": 3,
      "orderType": "DELIVERY",
      "tableId": null
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item(filler)",
    "request": {
      "item": "multi"
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "add_item",
    "request": {
      "item": "single",
      "q": 1
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "remove_item",
    "request": {
      "victim": "019ff4dc-82d1-7100-b850-bbb3246d4e92"
    },
    "ok": true
  },
  {
    "seq": 5,
    "action": "add_item",
    "request": {
      "item": "single",
      "q": 1
    },
    "ok": true
  },
  {
    "seq": 6,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 7,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4dc-82cd-79e0-9a5b-607f4f6c36b1 is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 8,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 9,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4dc-82cd-79e0-9a5b-607f4f6c36b1 has no unticketed, station-routed items to send"
    }
  }
]
```

### random-138 (seed 12483)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "no_station",
      "qty": 3,
      "orderType": "DELIVERY",
      "tableId": null
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item(filler)",
    "request": {
      "item": "no_station"
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "add_item(filler)",
    "request": {
      "item": "multi"
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "add_item(filler)",
    "request": {
      "item": "single"
    },
    "ok": true
  },
  {
    "seq": 5,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 6,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4dc-8c2f-7cf2-a990-cfd0bc54322c is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 7,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 8,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4dc-8c2f-7cf2-a990-cfd0bc54322c has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 9,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4dc-8c38-7303-a883-57246ccb346d",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 10,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4dc-8c38-7303-a883-574948513740",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  }
]
```

### random-139 (seed 12484)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "single2",
      "qty": 1,
      "orderType": "DELIVERY",
      "tableId": null
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item(filler)",
    "request": {
      "item": "no_station"
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 4,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4dc-9083-7633-a96d-85a0caec8bc0 is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 5,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 6,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4dc-9083-7633-a96d-85a0caec8bc0 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 7,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff4dc-909a-7a52-8a36-d71a73d53b09"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4dc-909a-7a52-8a36-d71a73d53b09 cannot transition from NEW to SERVED"
    }
  },
  {
    "seq": 8,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4dc-909a-7a52-8a36-d71a73d53b09",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  }
]
```

### random-140 (seed 12485)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "single2",
      "qty": 3,
      "orderType": "DINE_IN",
      "tableId": "0191a000-0000-7000-8000-000000000020"
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item",
    "request": {
      "item": "no_station",
      "q": 2
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "add_item",
    "request": {
      "item": "no_station",
      "q": 2
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "update_shape",
    "request": {
      "newType": "DINE_IN",
      "newTable": "0191a000-0000-7000-8000-000000000021"
    },
    "ok": true
  },
  {
    "seq": 5,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 6,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4dc-a05d-75a3-b815-027bf437c8cc is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 7,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 8,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4dc-a05d-75a3-b815-027bf437c8cc has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 9,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff4dc-a085-7ba0-aaae-c1361eed3e3d"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4dc-a085-7ba0-aaae-c1361eed3e3d cannot transition from NEW to SERVED"
    }
  },
  {
    "seq": 10,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4dc-a085-7ba0-aaae-c1361eed3e3d",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  }
]
```

### random-148 (seed 12493)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "no_station",
      "qty": 2,
      "orderType": "DELIVERY",
      "tableId": null
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item",
    "request": {
      "item": "multi",
      "q": 1
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 4,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4dd-5538-7223-8e73-ddbcc3bc1ba0 is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 5,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 6,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4dd-5538-7223-8e73-ddbcc3bc1ba0 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 7,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff4dd-5545-7442-9340-69cb62d1b998"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4dd-5545-7442-9340-69cb62d1b998 cannot transition from NEW to SERVED"
    }
  },
  {
    "seq": 8,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff4dd-5546-7251-a095-8ca3eae5652f"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4dd-5546-7251-a095-8ca3eae5652f cannot transition from NEW to SERVED"
    }
  },
  {
    "seq": 9,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4dd-5546-7251-a095-8ca3eae5652f",
      "status": "PREPARING"
    },
    "ok": true
  },
  {
    "seq": 10,
    "action": "illegal_transition_probe",
    "request": {
      "kotId": "019ff4dd-5546-7251-a095-8ca3eae5652f",
      "from": "PREPARING",
      "to": "SERVED"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4dd-5546-7251-a095-8ca3eae5652f cannot transition from PREPARING to SERVED"
    }
  }
]
```

### random-149 (seed 12494)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "single",
      "qty": 3,
      "orderType": "DELIVERY",
      "tableId": null
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item(filler)",
    "request": {
      "item": "single"
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "add_item",
    "request": {
      "item": "no_station",
      "q": 3
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "crash_and_recover(mid-draft)",
    "ok": true
  },
  {
    "seq": 5,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 6,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4dd-64b8-70c2-a6cc-072a2ce1aff6 is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 7,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 8,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4dd-64b8-70c2-a6cc-072a2ce1aff6 has no unticketed, station-routed items to send"
    }
  }
]
```

### random-150 (seed 12495)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "single",
      "qty": 3,
      "orderType": "DINE_IN",
      "tableId": "0191a000-0000-7000-8000-000000000021"
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item(filler)",
    "request": {
      "item": "no_station"
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "update_shape",
    "request": {
      "newType": "DELIVERY",
      "newTable": null
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "add_item(filler)",
    "request": {
      "item": "single2"
    },
    "ok": true
  },
  {
    "seq": 5,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 6,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4de-066d-78a3-8773-74e1e0834a1e is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 7,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 8,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4de-066d-78a3-8773-74e1e0834a1e has no unticketed, station-routed items to send"
    }
  }
]
```

### random-153 (seed 12498)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "single",
      "qty": 2,
      "orderType": "DINE_IN",
      "tableId": null
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item(filler)",
    "request": {
      "item": "no_station"
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "add_item(filler)",
    "request": {
      "item": "multi"
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 5,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4de-2e9b-7d61-9456-20d1576b42f3 is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 6,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 7,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4de-2e9b-7d61-9456-20d1576b42f3 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 8,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff4de-2ebb-7003-8649-6cab91b61c9a"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4de-2ebb-7003-8649-6cab91b61c9a cannot transition from NEW to SERVED"
    }
  },
  {
    "seq": 9,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4de-2ebb-7003-8649-6cab91b61c9a",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 10,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4de-2ebb-7003-8649-6cab91b61c9a",
      "status": "PREPARING"
    },
    "ok": true
  },
  {
    "seq": 11,
    "action": "illegal_transition_probe",
    "request": {
      "kotId": "019ff4de-2ebb-7003-8649-6cab91b61c9a",
      "from": "PREPARING",
      "to": "SERVED"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4de-2ebb-7003-8649-6cab91b61c9a cannot transition from PREPARING to SERVED"
    }
  },
  {
    "seq": 12,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff4de-2ebf-7eb1-ad72-442ea9fb2200"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4de-2ebf-7eb1-ad72-442ea9fb2200 cannot transition from NEW to SERVED"
    }
  },
  {
    "seq": 13,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4de-2ebf-7eb1-ad72-442ea9fb2200",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 14,
    "action": "illegal_transition_probe",
    "request": {
      "kotId": "019ff4de-2ebf-7eb1-ad72-442ea9fb2200",
      "from": "ACKNOWLEDGED",
      "to": "SERVED"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4de-2ebf-7eb1-ad72-442ea9fb2200 cannot transition from ACKNOWLEDGED to SERVED"
    }
  }
]
```

### random-156 (seed 12501)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "single2",
      "qty": 1,
      "orderType": "DELIVERY",
      "tableId": null
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item(filler)",
    "request": {
      "item": "no_station"
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "add_item",
    "request": {
      "item": "single",
      "q": 3
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "update_shape",
    "request": {
      "newType": "TAKEAWAY",
      "newTable": null
    },
    "ok": true
  },
  {
    "seq": 5,
    "action": "add_item(filler)",
    "request": {
      "item": "single2"
    },
    "ok": true
  },
  {
    "seq": 6,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 7,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4de-5453-7d02-a9bd-ca4c024c62ec is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 8,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 9,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4de-5453-7d02-a9bd-ca4c024c62ec has no unticketed, station-routed items to send"
    }
  }
]
```

### random-160 (seed 12505)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "single",
      "qty": 3,
      "orderType": "DELIVERY",
      "tableId": "0191a000-0000-7000-8000-000000000021"
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "update_shape",
    "request": {
      "newType": "DELIVERY",
      "newTable": null
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "add_item",
    "request": {
      "item": "no_station",
      "q": 2
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 5,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4de-f80d-7713-ab7d-0eff8a7a1f3c is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 6,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 7,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4de-f80d-7713-ab7d-0eff8a7a1f3c has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 8,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff4de-f815-7220-b572-67bea044fd36"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4de-f815-7220-b572-67bea044fd36 cannot transition from NEW to SERVED"
    }
  },
  {
    "seq": 9,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4de-f815-7220-b572-67bea044fd36",
      "status": "READY"
    },
    "ok": true
  }
]
```

### random-161 (seed 12506)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "single2",
      "qty": 3,
      "orderType": "DINE_IN",
      "tableId": null
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "update_shape",
    "request": {
      "newType": "TAKEAWAY",
      "newTable": null
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "add_item(filler)",
    "request": {
      "item": "single2"
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "update_shape",
    "request": {
      "newType": "TAKEAWAY",
      "newTable": null
    },
    "ok": true
  },
  {
    "seq": 5,
    "action": "add_item(filler)",
    "request": {
      "item": "no_station"
    },
    "ok": true
  },
  {
    "seq": 6,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 7,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4de-fa79-75b0-bd40-252f83fdbd4a is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 8,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 9,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4de-fa79-75b0-bd40-252f83fdbd4a has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 10,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4de-fa8d-7032-abb1-c1b1b17292d5",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 11,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4de-fa8d-7032-abb1-c1b1b17292d5",
      "status": "PREPARING"
    },
    "ok": true
  },
  {
    "seq": 12,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4de-fa8d-7032-abb1-c1b1b17292d5",
      "status": "READY"
    },
    "ok": true
  }
]
```

### random-163 (seed 12508)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "no_station",
      "qty": 2,
      "orderType": "TAKEAWAY",
      "tableId": "0191a000-0000-7000-8000-000000000020"
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item",
    "request": {
      "item": "single",
      "q": 3
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "add_item",
    "request": {
      "item": "single",
      "q": 1
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "add_item(filler)",
    "request": {
      "item": "single2"
    },
    "ok": true
  },
  {
    "seq": 5,
    "action": "add_item",
    "request": {
      "item": "no_station",
      "q": 3
    },
    "ok": true
  },
  {
    "seq": 6,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 7,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4df-1699-7453-b138-4be3c1b127a2 is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 8,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 9,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4df-1699-7453-b138-4be3c1b127a2 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 10,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4df-16a6-7070-adce-5f22dc4192fb",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 11,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4df-16a6-7070-adce-5f22dc4192fb",
      "status": "PREPARING"
    },
    "ok": true
  }
]
```

### random-167 (seed 12512)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "no_station",
      "qty": 1,
      "orderType": "DINE_IN",
      "tableId": "0191a000-0000-7000-8000-000000000020"
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "update_shape",
    "request": {
      "newType": "TAKEAWAY",
      "newTable": null
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "add_item",
    "request": {
      "item": "single2",
      "q": 3
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 5,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4df-37ab-7512-af9a-c3b18239687c is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 6,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 7,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4df-37ab-7512-af9a-c3b18239687c has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 8,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4df-37b9-7663-82bd-83df361532bb",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  }
]
```

### random-170 (seed 12515)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "single2",
      "qty": 1,
      "orderType": "TAKEAWAY",
      "tableId": null
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item(filler)",
    "request": {
      "item": "no_station"
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 4,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4df-56d2-75a3-a8fa-48caec2881f4 is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 5,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 6,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4df-56d2-75a3-a8fa-48caec2881f4 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 7,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff4df-56de-7a23-9df1-6a242c40fae8"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4df-56de-7a23-9df1-6a242c40fae8 cannot transition from NEW to SERVED"
    }
  },
  {
    "seq": 8,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4df-56de-7a23-9df1-6a242c40fae8",
      "status": "PREPARING"
    },
    "ok": true
  },
  {
    "seq": 9,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4df-56de-7a23-9df1-6a242c40fae8",
      "status": "SERVED"
    },
    "ok": true
  }
]
```

### random-172 (seed 12517)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "no_station",
      "qty": 2,
      "orderType": "DINE_IN",
      "tableId": null
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item",
    "request": {
      "item": "single2",
      "q": 3
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 4,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4df-680f-7d30-acc1-ee6123e107a3 is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 5,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 6,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4df-680f-7d30-acc1-ee6123e107a3 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 7,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff4df-681a-7ff0-9f26-7ab869a93186"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4df-681a-7ff0-9f26-7ab869a93186 cannot transition from NEW to SERVED"
    }
  }
]
```

### random-175 (seed 12520)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "multi",
      "qty": 3,
      "orderType": "DINE_IN",
      "tableId": null
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item(filler)",
    "request": {
      "item": "no_station"
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "add_item",
    "request": {
      "item": "no_station",
      "q": 1
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "add_item(filler)",
    "request": {
      "item": "single"
    },
    "ok": true
  },
  {
    "seq": 5,
    "action": "add_item",
    "request": {
      "item": "single",
      "q": 3
    },
    "ok": true
  },
  {
    "seq": 6,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 7,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4df-e6d0-7643-8cac-045358bd175a is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 8,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 9,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4df-e6d0-7643-8cac-045358bd175a has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 10,
    "action": "illegal_transition_probe",
    "request": {
      "kotId": "019ff4df-e6e0-7741-b18e-ec19b8504d53",
      "from": "ACKNOWLEDGED",
      "to": "SERVED"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4df-e6e0-7741-b18e-ec19b8504d53 cannot transition from ACKNOWLEDGED to SERVED"
    }
  },
  {
    "seq": 11,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff4df-e6e0-7741-b18e-ec321067cc3b"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4df-e6e0-7741-b18e-ec321067cc3b cannot transition from NEW to SERVED"
    }
  },
  {
    "seq": 12,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4df-e6e0-7741-b18e-ec321067cc3b",
      "status": "PREPARING"
    },
    "ok": true
  }
]
```

### random-178 (seed 12523)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "multi",
      "qty": 3,
      "orderType": "DINE_IN",
      "tableId": null
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item(filler)",
    "request": {
      "item": "no_station"
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "add_item(filler)",
    "request": {
      "item": "single2"
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 5,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4df-fa8c-7d30-b535-58987d13266e is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 6,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 7,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4df-fa8c-7d30-b535-58987d13266e has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 8,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4df-fa9a-7a03-923d-112fd7db8ca4",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 9,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4df-fa9b-7283-9e4e-786ffb31dc5b",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  }
]
```

### random-181 (seed 12526)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "single2",
      "qty": 2,
      "orderType": "TAKEAWAY",
      "tableId": "0191a000-0000-7000-8000-000000000021"
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item(filler)",
    "request": {
      "item": "single2"
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "add_item(filler)",
    "request": {
      "item": "single"
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "add_item(filler)",
    "request": {
      "item": "single2"
    },
    "ok": true
  },
  {
    "seq": 5,
    "action": "add_item",
    "request": {
      "item": "no_station",
      "q": 1
    },
    "ok": true
  },
  {
    "seq": 6,
    "action": "crash_and_recover(mid-draft)",
    "ok": true
  },
  {
    "seq": 7,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 8,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4e0-040a-70e0-afc2-472301e693aa is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 9,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 10,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4e0-040a-70e0-afc2-472301e693aa has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 11,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4e0-59ee-7411-8f0f-b40793d307a4",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 12,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4e0-59ee-7411-8f0f-b40793d307a4",
      "status": "PREPARING"
    },
    "ok": true
  }
]
```

### random-184 (seed 12529)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "no_station",
      "qty": 3,
      "orderType": "DELIVERY",
      "tableId": "0191a000-0000-7000-8000-000000000021"
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item(filler)",
    "request": {
      "item": "single"
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "add_item",
    "request": {
      "item": "no_station",
      "q": 3
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "remove_item",
    "request": {
      "victim": "019ff4e0-6164-72b2-a336-40dcd9e0b4b2"
    },
    "ok": true
  },
  {
    "seq": 5,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 6,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4e0-615c-7580-9e88-08649ee71238 is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 7,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 8,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4e0-615c-7580-9e88-08649ee71238 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 9,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff4e0-616a-76e0-80a7-0822aeecdfc0"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4e0-616a-76e0-80a7-0822aeecdfc0 cannot transition from NEW to SERVED"
    }
  }
]
```

### random-187 (seed 12532)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "multi",
      "qty": 1,
      "orderType": "DINE_IN",
      "tableId": null
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "add_item(filler)",
    "request": {
      "item": "multi"
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "add_item",
    "request": {
      "item": "multi",
      "q": 1
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "add_item(filler)",
    "request": {
      "item": "no_station"
    },
    "ok": true
  },
  {
    "seq": 5,
    "action": "add_item",
    "request": {
      "item": "multi",
      "q": 1
    },
    "ok": true
  },
  {
    "seq": 6,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 7,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4e0-af87-7712-bd6a-e8a4effd6b90 is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 8,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 9,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4e0-af87-7712-bd6a-e8a4effd6b90 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 10,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff4e0-af94-7ba0-ab77-488e10a50ed2"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4e0-af94-7ba0-ab77-488e10a50ed2 cannot transition from NEW to SERVED"
    }
  },
  {
    "seq": 11,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4e0-af94-7ba0-ab77-488e10a50ed2",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 12,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4e0-af94-7ba0-ab77-488e10a50ed2",
      "status": "PREPARING"
    },
    "ok": true
  },
  {
    "seq": 13,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff4e0-af94-7ba0-ab77-488e10a50ed2",
      "status": "READY"
    },
    "ok": true
  },
  {
    "seq": 14,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff4e0-af94-7ba0-ab77-48afaeebee3b"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4e0-af94-7ba0-ab77-48afaeebee3b cannot transition from NEW to SERVED"
    }
  }
]
```

### random-196 (seed 12541)

Broken invariants:
- **4_no_station_explicit**: zero-station-item-send: order mixed routable + no-station items; send_to_kitchen succeeded silently for the no-station line — no error, no per-item outcome field.

Action sequence:
```json
[
  {
    "seq": 1,
    "action": "create_draft",
    "request": {
      "item": "no_station",
      "qty": 3,
      "orderType": "TAKEAWAY",
      "tableId": null
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "update_shape",
    "request": {
      "newType": "DELIVERY",
      "newTable": null
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "update_shape",
    "request": {
      "newType": "DELIVERY",
      "newTable": null
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "update_shape",
    "request": {
      "newType": "TAKEAWAY",
      "newTable": null
    },
    "ok": true
  },
  {
    "seq": 5,
    "action": "add_item",
    "request": {
      "item": "single",
      "q": 3
    },
    "ok": true
  },
  {
    "seq": 6,
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 7,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff4e1-14db-7b32-9ddd-ec252b7e89ed is not amendable: status is CONFIRMED, not DRAFT"
    }
  },
  {
    "seq": 8,
    "action": "send_to_kitchen",
    "ok": true
  },
  {
    "seq": 9,
    "action": "send_to_kitchen(again)",
    "ok": false,
    "error": {
      "code": "NOTHING_TO_SEND_TO_KITCHEN",
      "message": "order 019ff4e1-14db-7b32-9ddd-ec252b7e89ed has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 10,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff4e1-14ec-78e1-b41b-9f142cce1892"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4e1-14ec-78e1-b41b-9f142cce1892 cannot transition from NEW to SERVED"
    }
  },
  {
    "seq": 11,
    "action": "illegal_transition_probe",
    "request": {
      "kotId": "019ff4e1-14ec-78e1-b41b-9f142cce1892",
      "from": "ACKNOWLEDGED",
      "to": "SERVED"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff4e1-14ec-78e1-b41b-9f142cce1892 cannot transition from ACKNOWLEDGED to SERVED"
    }
  }
]
```

## Findings (coverage gaps / product defects — not fixed by this track)

- COVERAGE GAP: modifiers are unreachable at the order-item level. commands::orders::NewOrderItemRequest (create_order/add_order_item's Tauri request shape) carries no modifiers field — apps/pos/src-tauri/src/commands/orders.rs states this explicitly ('this app's cart carries no modifiers yet'). MenuItemModifier rows exist and price deltas are seeded, but no shipped command can attach one to an order line, so 'add item with modifiers' cannot be exercised end to end.
- COVERAGE GAP: cancel_kitchen_items_with_outbox exists in edge/database but has no Tauri command — '#132-C' cancellation of items already sent to the kitchen is unreachable from the shipped surface. Not faked and not added here per track rules; recorded as a finding only.
- COVERAGE GAP: no shipped command can add an item to an order once it has left DRAFT (add_order_item requires DRAFT) — partial add-then-send / KOT '#132-A' amendments are unreachable from the shipped Tauri surface.
- PRODUCT DEFECT (named regression zero-station-item-send): send_order_to_kitchen_with_outbox silently skips a no-station line item when the order also has routable items — the cashier gets no indication that line never reached any kitchen screen. All-unrouted orders DO get an explicit NOTHING_TO_SEND_TO_KITCHEN error; only the mixed case is silent. Not fixed here per track rules — filed as a finding.

## Latency distribution (invariant 3: KOT reaches a subscribed KDS client)

Samples: 548
P50: 13ms, P95: 24ms, max: 27ms

## Latency distribution (invariant 8: KDS status change echoed POS-side)

Samples: 310
P50: 4ms, P95: 31ms, max: 41ms

## Crash-simulation scenarios

33 scenario(s) included a crash+recover step.
