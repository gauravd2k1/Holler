-- Milestone 0 skeleton migration: tenant hierarchy only.
-- Full contract-owned schema (with constraints/indexes tuned for the vertical
-- slice) lands in packages/contracts/ per Milestone 0.5 — see ADR-008.

CREATE TABLE organisations (
    id          UUID PRIMARY KEY,
    name        TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE brands (
    id              UUID PRIMARY KEY,
    organisation_id UUID NOT NULL REFERENCES organisations(id),
    name            TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE outlets (
    id          UUID PRIMARY KEY,
    brand_id    UUID NOT NULL REFERENCES brands(id),
    name        TEXT NOT NULL,
    timezone    TEXT NOT NULL DEFAULT 'Asia/Kolkata',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_brands_organisation_id ON brands(organisation_id);
CREATE INDEX idx_outlets_brand_id ON outlets(brand_id);
