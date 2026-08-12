# e2e-scenario-harness run report

Generated: 2026-08-11T20:00:16.703Z
Base seed: `424242` (reproduce a specific scenario with `--seed 424242`; each scenario's own seed is its `seed` field below and is independently reproducible via the same base + index).
Scenario count: 54

## Pass/fail per invariant

| Invariant | Checked | Passed | Failed | Unchecked |
|---|---|---|---|---|
| 1_state_machine | 54 | 54 | 0 | 0 |
| 2_kot_conservation | 45 | 45 | 0 | 9 |
| 3_kds_fidelity | 45 | 45 | 0 | 9 |
| 4_no_station_explicit | 26 | 9 | 17 | 28 |
| 5_money | 54 | 54 | 0 | 0 |
| 6_durability | 7 | 7 | 0 | 47 |
| 7_outbox | 54 | 54 | 0 | 0 |
| 8_status_echo | 38 | 38 | 0 | 16 |

## Fatal errors (harness/process-level, not invariant failures): 0


## Every scenario with at least one failed invariant (full replayable action sequence)

### stuck-draft-rescue (seed 433246)

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
      "message": "order 019ff267-95a4-7c81-ab0b-02b2bd91352a is not amendable: status is CONFIRMED, not DRAFT"
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
      "message": "order 019ff267-95a4-7c81-ab0b-02b2bd91352a has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 8,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff267-acdd-7db2-9b8d-d5593d8131a1",
      "status": "READY"
    },
    "ok": true
  },
  {
    "seq": 9,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff267-acdd-7db2-9b8d-d5593d8131a1",
      "status": "SERVED"
    },
    "ok": true
  }
]
```

### random-3 (seed 424245)

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
      "tableId": "0191a000-0000-7000-8000-000000000020"
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
      "item": "multi",
      "q": 1
    },
    "ok": true
  },
  {
    "seq": 4,
    "action": "remove_item",
    "request": {
      "victim": "019ff267-b633-7040-810a-ff10fe27a43c"
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
      "message": "order 019ff267-b62f-7982-93b3-03ca7c9bf079 is not amendable: status is CONFIRMED, not DRAFT"
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
      "message": "order 019ff267-b62f-7982-93b3-03ca7c9bf079 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 9,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff267-b637-7073-a9a8-14d3a595fd47",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  }
]
```

### random-7 (seed 424249)

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
    "action": "add_item",
    "request": {
      "item": "no_station",
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
      "message": "order 019ff267-e2a8-7d02-9311-e6d3bf2561d2 is not amendable: status is CONFIRMED, not DRAFT"
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
      "message": "order 019ff267-e2a8-7d02-9311-e6d3bf2561d2 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 10,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff267-e2b8-7962-b702-a63bcf57cbf6",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 11,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff267-e2b8-7962-b702-a63bcf57cbf6",
      "status": "READY"
    },
    "ok": true
  },
  {
    "seq": 12,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff267-e2b8-7962-b702-a65cbfd4edf1",
      "status": "PREPARING"
    },
    "ok": true
  }
]
```

### random-11 (seed 424253)

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
      "tableId": "0191a000-0000-7000-8000-000000000020"
    },
    "ok": true
  },
  {
    "seq": 2,
    "action": "update_shape",
    "request": {
      "newType": "DINE_IN",
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
      "message": "order 019ff267-ebdc-7a12-af88-5e8010adf138 is not amendable: status is CONFIRMED, not DRAFT"
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
      "message": "order 019ff267-ebdc-7a12-af88-5e8010adf138 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 8,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff267-ebe7-73f0-a526-807c83913716"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff267-ebe7-73f0-a526-807c83913716 cannot transition from NEW to SERVED"
    }
  },
  {
    "seq": 9,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff267-ebe7-73f0-a526-807c83913716",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 10,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff267-ebe7-73f0-a526-807c83913716",
      "status": "PREPARING"
    },
    "ok": true
  },
  {
    "seq": 11,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff267-ebe7-73f0-a526-807c83913716",
      "status": "SERVED"
    },
    "ok": true
  }
]
```

### random-13 (seed 424255)

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
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 4,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff268-270f-7d10-b321-10a1cba900ce is not amendable: status is CONFIRMED, not DRAFT"
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
      "message": "order 019ff268-270f-7d10-b321-10a1cba900ce has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 7,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff268-271c-7190-9e77-336251510eeb"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff268-271c-7190-9e77-336251510eeb cannot transition from NEW to SERVED"
    }
  }
]
```

### random-15 (seed 424257)

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
      "newType": "TAKEAWAY",
      "newTable": null
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
      "message": "order 019ff268-2bb6-7331-ad4e-00d2281019c1 is not amendable: status is CONFIRMED, not DRAFT"
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
      "message": "order 019ff268-2bb6-7331-ad4e-00d2281019c1 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 8,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff268-2bc4-7bf1-b3c0-d84f75c08157",
      "status": "READY"
    },
    "ok": true
  }
]
```

### random-17 (seed 424259)

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
      "message": "order 019ff268-3188-7e91-9940-c4256e1f252b is not amendable: status is CONFIRMED, not DRAFT"
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
      "message": "order 019ff268-3188-7e91-9940-c4256e1f252b has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 10,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff268-31ab-7440-877b-1b474965ab08",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 11,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff268-31ab-7440-877b-1b474965ab08",
      "status": "PREPARING"
    },
    "ok": true
  },
  {
    "seq": 12,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff268-31ac-78a0-bbce-1dfeaf82b2bb"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff268-31ac-78a0-bbce-1dfeaf82b2bb cannot transition from NEW to SERVED"
    }
  },
  {
    "seq": 13,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff268-31ac-78a0-bbce-1dfeaf82b2bb",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  }
]
```

### random-25 (seed 424267)

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
      "tableId": "0191a000-0000-7000-8000-000000000020"
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
    "action": "add_item",
    "request": {
      "item": "single2",
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
      "message": "order 019ff268-9f0d-7d51-98a9-597d00eb39c8 is not amendable: status is CONFIRMED, not DRAFT"
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
      "message": "order 019ff268-9f0d-7d51-98a9-597d00eb39c8 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 8,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff268-9f1b-7063-8bbe-85caf23f3ab9"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff268-9f1b-7063-8bbe-85caf23f3ab9 cannot transition from NEW to SERVED"
    }
  },
  {
    "seq": 9,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff268-9f1b-7063-8bbe-85caf23f3ab9",
      "status": "READY"
    },
    "ok": true
  },
  {
    "seq": 10,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff268-9f1b-7063-8bbe-85caf23f3ab9",
      "status": "SERVED"
    },
    "ok": true
  },
  {
    "seq": 11,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff268-9f1b-7063-8bbe-85e0c848b157",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  }
]
```

### random-33 (seed 424275)

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
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 4,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff268-f6fe-7e22-a532-8ad0fe6ca360 is not amendable: status is CONFIRMED, not DRAFT"
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
      "message": "order 019ff268-f6fe-7e22-a532-8ad0fe6ca360 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 7,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff268-f704-78e2-bc72-3957829222c1"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff268-f704-78e2-bc72-3957829222c1 cannot transition from NEW to SERVED"
    }
  },
  {
    "seq": 8,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff268-f704-78e2-bc72-3957829222c1",
      "status": "PREPARING"
    },
    "ok": true
  },
  {
    "seq": 9,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff268-f704-78e2-bc72-3971c0df8ad9",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 10,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff268-f704-78e2-bc72-3971c0df8ad9",
      "status": "PREPARING"
    },
    "ok": true
  }
]
```

### random-35 (seed 424277)

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
      "newType": "DELIVERY",
      "newTable": null
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
      "message": "order 019ff268-fbb7-7be0-89ed-0242827a2ce1 is not amendable: status is CONFIRMED, not DRAFT"
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
      "message": "order 019ff268-fbb7-7be0-89ed-0242827a2ce1 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 10,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff268-fbc0-73c3-bbc2-ab5e037b5522",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 11,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff268-fbc0-73c3-bbc2-ab5e037b5522",
      "status": "PREPARING"
    },
    "ok": true
  },
  {
    "seq": 12,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff268-fbc0-73c3-bbc2-ab5e037b5522",
      "status": "READY"
    },
    "ok": true
  }
]
```

### random-36 (seed 424278)

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
      "message": "order 019ff268-fe03-7d31-8c17-b18974844564 is not amendable: status is CONFIRMED, not DRAFT"
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
      "message": "order 019ff268-fe03-7d31-8c17-b18974844564 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 8,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff268-fe09-73e3-b020-d9da43168ab5"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff268-fe09-73e3-b020-d9da43168ab5 cannot transition from NEW to SERVED"
    }
  },
  {
    "seq": 9,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff268-fe09-73e3-b020-d9da43168ab5",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  }
]
```

### random-37 (seed 424279)

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
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 4,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff269-00bb-74e1-b605-817e3d974c64 is not amendable: status is CONFIRMED, not DRAFT"
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
      "message": "order 019ff269-00bb-74e1-b605-817e3d974c64 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 7,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff269-00c5-7220-9eac-9b0993b793ef",
      "status": "PREPARING"
    },
    "ok": true
  },
  {
    "seq": 8,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff269-00c6-7343-b783-17f282b19a93"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff269-00c6-7343-b783-17f282b19a93 cannot transition from NEW to SERVED"
    }
  }
]
```

### random-39 (seed 424281)

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
      "newType": "TAKEAWAY",
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
      "message": "order 019ff269-11df-7042-9cfc-bea3fdcecb1a is not amendable: status is CONFIRMED, not DRAFT"
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
      "message": "order 019ff269-11df-7042-9cfc-bea3fdcecb1a has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 9,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff269-11ec-7372-94ef-cd17eb6b09e2"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff269-11ec-7372-94ef-cd17eb6b09e2 cannot transition from NEW to SERVED"
    }
  },
  {
    "seq": 10,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff269-11ec-7372-94ef-cd17eb6b09e2",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 11,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff269-11ec-7372-94ef-cd17eb6b09e2",
      "status": "PREPARING"
    },
    "ok": true
  }
]
```

### random-41 (seed 424283)

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
      "message": "order 019ff269-1778-76e1-9442-9d9782186da5 is not amendable: status is CONFIRMED, not DRAFT"
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
      "message": "order 019ff269-1778-76e1-9442-9d9782186da5 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 9,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff269-1784-7de3-ac61-cbd31707f714",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 10,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff269-1784-7de3-ac61-cbd31707f714",
      "status": "SERVED"
    },
    "ok": true
  },
  {
    "seq": 11,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff269-1784-7de3-ac61-cbf446dbbd6b",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 12,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff269-1784-7de3-ac61-cbf446dbbd6b",
      "status": "PREPARING"
    },
    "ok": true
  },
  {
    "seq": 13,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff269-1784-7de3-ac61-cbf446dbbd6b",
      "status": "READY"
    },
    "ok": true
  }
]
```

### random-42 (seed 424284)

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
    "action": "confirm",
    "ok": true
  },
  {
    "seq": 4,
    "action": "attempt_add_after_confirm(coverage-probe)",
    "ok": false,
    "error": {
      "code": "ORDER_NOT_DRAFT",
      "message": "order 019ff269-1a4a-7183-9f25-91e34a0868d6 is not amendable: status is CONFIRMED, not DRAFT"
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
      "message": "order 019ff269-1a4a-7183-9f25-91e34a0868d6 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 7,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff269-1a53-78e2-ac1d-b0e2445535ca"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff269-1a53-78e2-ac1d-b0e2445535ca cannot transition from NEW to SERVED"
    }
  },
  {
    "seq": 8,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff269-1a53-78e2-ac1d-b0e2445535ca",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 9,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff269-1a53-78e2-ac1d-b0e2445535ca",
      "status": "READY"
    },
    "ok": true
  }
]
```

### random-44 (seed 424286)

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
      "item": "single"
    },
    "ok": true
  },
  {
    "seq": 3,
    "action": "remove_item",
    "request": {
      "victim": "019ff269-1fc4-7913-bf66-cbe77b5ce48e"
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
      "message": "order 019ff269-1fc0-7440-8a36-06ebafb5caf5 is not amendable: status is CONFIRMED, not DRAFT"
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
      "message": "order 019ff269-1fc0-7440-8a36-06ebafb5caf5 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 10,
    "action": "illegal_transition_probe(NEW->SERVED)",
    "request": {
      "kotId": "019ff269-1fcd-79f3-8bed-27f9ade03d6a"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff269-1fcd-79f3-8bed-27f9ade03d6a cannot transition from NEW to SERVED"
    }
  },
  {
    "seq": 11,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff269-1fcd-79f3-8bed-27f9ade03d6a",
      "status": "PREPARING"
    },
    "ok": true
  },
  {
    "seq": 12,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff269-1fcd-79f3-8bed-27f9ade03d6a",
      "status": "READY"
    },
    "ok": true
  },
  {
    "seq": 13,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff269-1fcd-79f3-8bed-281beba601f4",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  },
  {
    "seq": 14,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff269-1fcd-79f3-8bed-281beba601f4",
      "status": "PREPARING"
    },
    "ok": true
  },
  {
    "seq": 15,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff269-1fcd-79f3-8bed-281beba601f4",
      "status": "READY"
    },
    "ok": true
  }
]
```

### random-48 (seed 424290)

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
    "action": "add_item",
    "request": {
      "item": "no_station",
      "q": 1
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
      "message": "order 019ff269-4259-7db0-acf3-706dcff22d69 is not amendable: status is CONFIRMED, not DRAFT"
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
      "message": "order 019ff269-4259-7db0-acf3-706dcff22d69 has no unticketed, station-routed items to send"
    }
  },
  {
    "seq": 9,
    "action": "illegal_transition_probe",
    "request": {
      "kotId": "019ff269-4267-7b03-b958-7fdd3835fcdc",
      "from": "ACKNOWLEDGED",
      "to": "SERVED"
    },
    "ok": false,
    "error": {
      "code": "ILLEGAL_KOT_STATUS_TRANSITION",
      "message": "kot 019ff269-4267-7b03-b958-7fdd3835fcdc cannot transition from ACKNOWLEDGED to SERVED"
    }
  },
  {
    "seq": 10,
    "action": "transition_kot(pos)",
    "request": {
      "kotId": "019ff269-4268-7fd3-9066-89accb0b647b",
      "status": "ACKNOWLEDGED"
    },
    "ok": true
  }
]
```

## Findings (coverage gaps / product defects — not fixed by this track)

- COVERAGE GAP: modifiers are unreachable at the order-item level. commands::orders::NewOrderItemRequest (create_order/add_order_item's Tauri request shape) carries no modifiers field — apps/pos/src-tauri/src/commands/orders.rs states this explicitly ('this app's cart carries no modifiers yet'). MenuItemModifier rows exist and price deltas are seeded, but no shipped command can attach one to an order line, so 'add item with modifiers' cannot be exercised end to end.
- COVERAGE GAP: cancel_kitchen_items_with_outbox exists in edge/database but has no Tauri command — '#132-C' cancellation of items already sent to the kitchen is unreachable from the shipped surface. Not faked and not added here per track rules; recorded as a finding only.
- COVERAGE GAP: no shipped command can add an item to an order once it has left DRAFT (add_order_item requires DRAFT) — partial add-then-send / KOT '#132-A' amendments are unreachable from the shipped Tauri surface.
- PRODUCT DEFECT (named regression zero-station-item-send): send_order_to_kitchen_with_outbox silently skips a no-station line item when the order also has routable items — the cashier gets no indication that line never reached any kitchen screen. All-unrouted orders DO get an explicit NOTHING_TO_SEND_TO_KITCHEN error; only the mixed case is silent. Not fixed here per track rules — filed as a finding.

## Latency distribution (invariant 3: KOT reaches a subscribed KDS client)

Samples: 148
P50: 13ms, P95: 24ms, max: 26ms

## Latency distribution (invariant 8: KDS status change echoed POS-side)

Samples: 89
P50: 2ms, P95: 29ms, max: 31ms

## Crash-simulation scenarios

7 scenario(s) included a crash+recover step.
