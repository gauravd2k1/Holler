package compliance

import (
	"context"
	"errors"
	"fmt"

	"github.com/jackc/pgx/v5"

	contracts "github.com/holler/contracts"

	"github.com/holler/backend/internal/platform/httpx"
	"github.com/holler/backend/internal/platform/postgres"
	"github.com/holler/backend/internal/platform/storage"
)

// Repository is the persistence boundary for the T13 config write path:
// compliance_version, tax_profile, tax_rule, invoice_series,
// discount_definition and outlet_fiscal_profile — every CLOUD_TO_EDGE
// aggregate ADR-016 assigns to this context. Service depends on this
// interface, never on pgx directly (CLAUDE.md §Coding rules).
type Repository interface {
	WithTx(ctx context.Context, fn func(tx pgx.Tx) error) error

	// BumpOutletConfigVersion increments outlet.config_version by exactly
	// one, mirroring backend/internal/tables and backend/internal/kitchen.
	BumpOutletConfigVersion(ctx context.Context, tx pgx.Tx, outletID string) (int, error)

	// OutletBelongsToTenant scopes every write and every sync-bundle read to
	// the caller's own tenant — never trusting a caller-supplied outlet_id
	// on its own (contract review rubric: uniqueness AND authorization are
	// tenant-scoped, never global).
	OutletBelongsToTenant(ctx context.Context, tenantID, outletID string) (bool, error)

	InsertComplianceVersion(ctx context.Context, tx pgx.Tx, cv contracts.ComplianceVersion) error
	GetComplianceVersion(ctx context.Context, id string) (contracts.ComplianceVersion, error)
	ComplianceVersionsSince(ctx context.Context, outletID string, sinceVersion int) ([]contracts.ComplianceVersion, error)

	InsertTaxProfile(ctx context.Context, tx pgx.Tx, tp contracts.TaxProfile) error
	GetTaxProfile(ctx context.Context, id string) (contracts.TaxProfile, error)
	TaxProfilesSince(ctx context.Context, outletID string, sinceVersion int) ([]contracts.TaxProfile, error)
	SetTaxProfileActive(ctx context.Context, tx pgx.Tx, id string, isActive bool, configVersion int) error

	InsertTaxRule(ctx context.Context, tx pgx.Tx, tr contracts.TaxRule) error
	TaxRulesSince(ctx context.Context, outletID string, sinceVersion int) ([]contracts.TaxRule, error)

	InsertInvoiceSeries(ctx context.Context, tx pgx.Tx, s contracts.InvoiceSeries) error
	GetInvoiceSeries(ctx context.Context, id string) (contracts.InvoiceSeries, error)
	InvoiceSeriesSince(ctx context.Context, outletID string, sinceVersion int) ([]contracts.InvoiceSeries, error)
	SetInvoiceSeriesActive(ctx context.Context, tx pgx.Tx, id string, isActive bool, configVersion int) error

	InsertDiscountDefinition(ctx context.Context, tx pgx.Tx, d contracts.DiscountDefinition) error
	GetDiscountDefinition(ctx context.Context, id string) (contracts.DiscountDefinition, error)
	DiscountDefinitionsSince(ctx context.Context, outletID string, sinceVersion int) ([]contracts.DiscountDefinition, error)
	SetDiscountDefinitionActive(ctx context.Context, tx pgx.Tx, id string, isActive bool, configVersion int) error

	InsertFiscalProfile(ctx context.Context, tx pgx.Tx, fp contracts.OutletFiscalProfile) error
	// CurrentFiscalProfile returns the row with the latest effective_from,
	// nil if outletID has none configured yet.
	CurrentFiscalProfile(ctx context.Context, outletID string) (*contracts.OutletFiscalProfile, error)
}

type pgRepository struct {
	pool postgres.Pool
}

// NewRepository returns a Repository backed by a live PostgreSQL pool.
func NewRepository(pool postgres.Pool) Repository {
	return &pgRepository{pool: pool}
}

func (r *pgRepository) WithTx(ctx context.Context, fn func(tx pgx.Tx) error) error {
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return fmt.Errorf("compliance: begin tx: %w", err)
	}
	if err := fn(tx); err != nil {
		_ = tx.Rollback(ctx)
		return err
	}
	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("compliance: commit tx: %w", err)
	}
	return nil
}

func (r *pgRepository) BumpOutletConfigVersion(ctx context.Context, tx pgx.Tx, outletID string) (int, error) {
	var newVersion int
	err := tx.QueryRow(ctx,
		`UPDATE outlet SET config_version = config_version + 1, updated_at = now()
		 WHERE id = $1 RETURNING config_version`,
		outletID,
	).Scan(&newVersion)
	if errors.Is(err, pgx.ErrNoRows) {
		return 0, fmt.Errorf("%w: outlet %s", httpx.ErrNotFound, outletID)
	}
	if err != nil {
		return 0, fmt.Errorf("compliance: bumping outlet config_version: %w", err)
	}
	return newVersion, nil
}

func (r *pgRepository) OutletBelongsToTenant(ctx context.Context, tenantID, outletID string) (bool, error) {
	var exists bool
	err := r.pool.QueryRow(ctx,
		`SELECT EXISTS(
			SELECT 1 FROM outlet o JOIN brand b ON b.id = o.brand_id
			WHERE o.id = $1 AND b.tenant_id = $2
		 )`,
		outletID, tenantID,
	).Scan(&exists)
	if err != nil {
		return false, fmt.Errorf("compliance: checking outlet tenancy: %w", err)
	}
	return exists, nil
}

// --- compliance_version ------------------------------------------------

func (r *pgRepository) InsertComplianceVersion(ctx context.Context, tx pgx.Tx, cv contracts.ComplianceVersion) error {
	_, err := tx.Exec(ctx,
		`INSERT INTO compliance_version (id, outlet_id, label, effective_from, notes, config_version)
		 VALUES ($1,$2,$3,$4,$5,$6)`,
		cv.ID, cv.OutletID, cv.Label, cv.EffectiveFrom, cv.Notes, cv.ConfigVersion,
	)
	if err != nil {
		if storage.IsUniqueViolation(err) {
			return fmt.Errorf("%w: compliance version label %q already exists in this outlet", httpx.ErrConflict, cv.Label)
		}
		return fmt.Errorf("compliance: inserting compliance_version: %w", err)
	}
	return nil
}

func (r *pgRepository) GetComplianceVersion(ctx context.Context, id string) (contracts.ComplianceVersion, error) {
	var cv contracts.ComplianceVersion
	err := r.pool.QueryRow(ctx,
		`SELECT id, outlet_id, label, effective_from, notes, config_version
		 FROM compliance_version WHERE id = $1`,
		id,
	).Scan(&cv.ID, &cv.OutletID, &cv.Label, &cv.EffectiveFrom, &cv.Notes, &cv.ConfigVersion)
	if errors.Is(err, pgx.ErrNoRows) {
		return contracts.ComplianceVersion{}, fmt.Errorf("%w: compliance version %s", httpx.ErrNotFound, id)
	}
	if err != nil {
		return contracts.ComplianceVersion{}, fmt.Errorf("compliance: getting compliance_version: %w", err)
	}
	cv.SchemaVersion = 1
	return cv, nil
}

func (r *pgRepository) ComplianceVersionsSince(ctx context.Context, outletID string, sinceVersion int) ([]contracts.ComplianceVersion, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT id, outlet_id, label, effective_from, notes, config_version
		 FROM compliance_version WHERE outlet_id = $1 AND config_version > $2 ORDER BY effective_from`,
		outletID, sinceVersion,
	)
	if err != nil {
		return nil, fmt.Errorf("compliance: listing compliance_version: %w", err)
	}
	defer rows.Close()

	var out []contracts.ComplianceVersion
	for rows.Next() {
		var cv contracts.ComplianceVersion
		if err := rows.Scan(&cv.ID, &cv.OutletID, &cv.Label, &cv.EffectiveFrom, &cv.Notes, &cv.ConfigVersion); err != nil {
			return nil, fmt.Errorf("compliance: scanning compliance_version: %w", err)
		}
		cv.SchemaVersion = 1
		out = append(out, cv)
	}
	return out, rows.Err()
}

// --- tax_profile ---------------------------------------------------------

func (r *pgRepository) InsertTaxProfile(ctx context.Context, tx pgx.Tx, tp contracts.TaxProfile) error {
	_, err := tx.Exec(ctx,
		`INSERT INTO tax_profile (id, outlet_id, code, name, pricing_mode, is_default, is_active, config_version)
		 VALUES ($1,$2,$3,$4,$5,$6,$7,$8)`,
		tp.ID, tp.OutletID, tp.Code, tp.Name, string(tp.PricingMode), tp.IsDefault, tp.IsActive, tp.ConfigVersion,
	)
	if err != nil {
		if storage.IsUniqueViolation(err) {
			return fmt.Errorf("%w: tax profile code %q already exists in this outlet", httpx.ErrConflict, tp.Code)
		}
		return fmt.Errorf("compliance: inserting tax_profile: %w", err)
	}
	return nil
}

func (r *pgRepository) GetTaxProfile(ctx context.Context, id string) (contracts.TaxProfile, error) {
	var tp contracts.TaxProfile
	var pricingMode string
	err := r.pool.QueryRow(ctx,
		`SELECT id, outlet_id, code, name, pricing_mode, is_default, is_active, config_version
		 FROM tax_profile WHERE id = $1`,
		id,
	).Scan(&tp.ID, &tp.OutletID, &tp.Code, &tp.Name, &pricingMode, &tp.IsDefault, &tp.IsActive, &tp.ConfigVersion)
	if errors.Is(err, pgx.ErrNoRows) {
		return contracts.TaxProfile{}, fmt.Errorf("%w: tax profile %s", httpx.ErrNotFound, id)
	}
	if err != nil {
		return contracts.TaxProfile{}, fmt.Errorf("compliance: getting tax_profile: %w", err)
	}
	tp.PricingMode = contracts.PricingMode(pricingMode)
	tp.SchemaVersion = 1
	return tp, nil
}

func (r *pgRepository) TaxProfilesSince(ctx context.Context, outletID string, sinceVersion int) ([]contracts.TaxProfile, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT id, outlet_id, code, name, pricing_mode, is_default, is_active, config_version
		 FROM tax_profile WHERE outlet_id = $1 AND config_version > $2 ORDER BY code`,
		outletID, sinceVersion,
	)
	if err != nil {
		return nil, fmt.Errorf("compliance: listing tax_profile: %w", err)
	}
	defer rows.Close()

	var out []contracts.TaxProfile
	for rows.Next() {
		var tp contracts.TaxProfile
		var pricingMode string
		if err := rows.Scan(&tp.ID, &tp.OutletID, &tp.Code, &tp.Name, &pricingMode, &tp.IsDefault, &tp.IsActive, &tp.ConfigVersion); err != nil {
			return nil, fmt.Errorf("compliance: scanning tax_profile: %w", err)
		}
		tp.PricingMode = contracts.PricingMode(pricingMode)
		tp.SchemaVersion = 1
		out = append(out, tp)
	}
	return out, rows.Err()
}

func (r *pgRepository) SetTaxProfileActive(ctx context.Context, tx pgx.Tx, id string, isActive bool, configVersion int) error {
	tag, err := tx.Exec(ctx,
		`UPDATE tax_profile SET is_active = $1, config_version = $2 WHERE id = $3`,
		isActive, configVersion, id,
	)
	if err != nil {
		return fmt.Errorf("compliance: updating tax_profile active state: %w", err)
	}
	if tag.RowsAffected() == 0 {
		return fmt.Errorf("%w: tax profile %s", httpx.ErrNotFound, id)
	}
	return nil
}

// --- tax_rule (child of tax_profile; not independently addressable) ------

func (r *pgRepository) InsertTaxRule(ctx context.Context, tx pgx.Tx, tr contracts.TaxRule) error {
	_, err := tx.Exec(ctx,
		`INSERT INTO tax_rule (id, tax_profile_id, compliance_version_id, component, rate_bps, effective_from, effective_to, config_version)
		 VALUES ($1,$2,$3,$4,$5,$6,$7,$8)`,
		tr.ID, tr.TaxProfileID, tr.ComplianceVersionID, string(tr.Component), tr.RateBps, tr.EffectiveFrom, tr.EffectiveTo, tr.ConfigVersion,
	)
	if err != nil {
		return fmt.Errorf("compliance: inserting tax_rule: %w", err)
	}
	return nil
}

func (r *pgRepository) TaxRulesSince(ctx context.Context, outletID string, sinceVersion int) ([]contracts.TaxRule, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT tr.id, tr.tax_profile_id, tr.compliance_version_id, tr.component, tr.rate_bps,
		        tr.effective_from, tr.effective_to, tr.config_version
		 FROM tax_rule tr
		 JOIN tax_profile tp ON tp.id = tr.tax_profile_id
		 WHERE tp.outlet_id = $1 AND tr.config_version > $2
		 ORDER BY tr.effective_from`,
		outletID, sinceVersion,
	)
	if err != nil {
		return nil, fmt.Errorf("compliance: listing tax_rule: %w", err)
	}
	defer rows.Close()

	var out []contracts.TaxRule
	for rows.Next() {
		var tr contracts.TaxRule
		var component string
		if err := rows.Scan(&tr.ID, &tr.TaxProfileID, &tr.ComplianceVersionID, &component, &tr.RateBps, &tr.EffectiveFrom, &tr.EffectiveTo, &tr.ConfigVersion); err != nil {
			return nil, fmt.Errorf("compliance: scanning tax_rule: %w", err)
		}
		tr.Component = contracts.TaxComponent(component)
		tr.SchemaVersion = 1
		out = append(out, tr)
	}
	return out, rows.Err()
}

// --- invoice_series --------------------------------------------------------

func (r *pgRepository) InsertInvoiceSeries(ctx context.Context, tx pgx.Tx, s contracts.InvoiceSeries) error {
	_, err := tx.Exec(ctx,
		`INSERT INTO invoice_series (id, outlet_id, code, prefix_template, reset_policy, padding_width, is_active, config_version)
		 VALUES ($1,$2,$3,$4,$5,$6,$7,$8)`,
		s.ID, s.OutletID, s.Code, s.PrefixTemplate, string(s.ResetPolicy), s.PaddingWidth, s.IsActive, s.ConfigVersion,
	)
	if err != nil {
		if storage.IsUniqueViolation(err) {
			return fmt.Errorf("%w: invoice series code %q already exists in this outlet", httpx.ErrConflict, s.Code)
		}
		return fmt.Errorf("compliance: inserting invoice_series: %w", err)
	}
	return nil
}

func (r *pgRepository) GetInvoiceSeries(ctx context.Context, id string) (contracts.InvoiceSeries, error) {
	var s contracts.InvoiceSeries
	var resetPolicy string
	err := r.pool.QueryRow(ctx,
		`SELECT id, outlet_id, code, prefix_template, reset_policy, padding_width, is_active, config_version
		 FROM invoice_series WHERE id = $1`,
		id,
	).Scan(&s.ID, &s.OutletID, &s.Code, &s.PrefixTemplate, &resetPolicy, &s.PaddingWidth, &s.IsActive, &s.ConfigVersion)
	if errors.Is(err, pgx.ErrNoRows) {
		return contracts.InvoiceSeries{}, fmt.Errorf("%w: invoice series %s", httpx.ErrNotFound, id)
	}
	if err != nil {
		return contracts.InvoiceSeries{}, fmt.Errorf("compliance: getting invoice_series: %w", err)
	}
	s.ResetPolicy = contracts.SequenceResetPolicy(resetPolicy)
	s.SchemaVersion = 1
	return s, nil
}

func (r *pgRepository) InvoiceSeriesSince(ctx context.Context, outletID string, sinceVersion int) ([]contracts.InvoiceSeries, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT id, outlet_id, code, prefix_template, reset_policy, padding_width, is_active, config_version
		 FROM invoice_series WHERE outlet_id = $1 AND config_version > $2 ORDER BY code`,
		outletID, sinceVersion,
	)
	if err != nil {
		return nil, fmt.Errorf("compliance: listing invoice_series: %w", err)
	}
	defer rows.Close()

	var out []contracts.InvoiceSeries
	for rows.Next() {
		var s contracts.InvoiceSeries
		var resetPolicy string
		if err := rows.Scan(&s.ID, &s.OutletID, &s.Code, &s.PrefixTemplate, &resetPolicy, &s.PaddingWidth, &s.IsActive, &s.ConfigVersion); err != nil {
			return nil, fmt.Errorf("compliance: scanning invoice_series: %w", err)
		}
		s.ResetPolicy = contracts.SequenceResetPolicy(resetPolicy)
		s.SchemaVersion = 1
		out = append(out, s)
	}
	return out, rows.Err()
}

func (r *pgRepository) SetInvoiceSeriesActive(ctx context.Context, tx pgx.Tx, id string, isActive bool, configVersion int) error {
	tag, err := tx.Exec(ctx,
		`UPDATE invoice_series SET is_active = $1, config_version = $2 WHERE id = $3`,
		isActive, configVersion, id,
	)
	if err != nil {
		return fmt.Errorf("compliance: updating invoice_series active state: %w", err)
	}
	if tag.RowsAffected() == 0 {
		return fmt.Errorf("%w: invoice series %s", httpx.ErrNotFound, id)
	}
	return nil
}

// --- discount_definition ----------------------------------------------------

func (r *pgRepository) InsertDiscountDefinition(ctx context.Context, tx pgx.Tx, d contracts.DiscountDefinition) error {
	_, err := tx.Exec(ctx,
		`INSERT INTO discount_definition (
			id, outlet_id, code, name, scope, method, value_bps, value_paise,
			max_discount_paise, required_permission, requires_reason, is_active,
			effective_from, effective_to, config_version
		 ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)`,
		d.ID, d.OutletID, d.Code, d.Name, string(d.Scope), string(d.Method), d.ValueBps, d.ValuePaise,
		d.MaxDiscountPaise, d.RequiredPermission, d.RequiresReason, d.IsActive,
		d.EffectiveFrom, d.EffectiveTo, d.ConfigVersion,
	)
	if err != nil {
		if storage.IsUniqueViolation(err) {
			return fmt.Errorf("%w: discount code %q already exists in this outlet", httpx.ErrConflict, d.Code)
		}
		return fmt.Errorf("compliance: inserting discount_definition: %w", err)
	}
	return nil
}

func (r *pgRepository) GetDiscountDefinition(ctx context.Context, id string) (contracts.DiscountDefinition, error) {
	var d contracts.DiscountDefinition
	var scope, method string
	err := r.pool.QueryRow(ctx,
		`SELECT id, outlet_id, code, name, scope, method, value_bps, value_paise,
		        max_discount_paise, required_permission, requires_reason, is_active,
		        effective_from, effective_to, config_version
		 FROM discount_definition WHERE id = $1`,
		id,
	).Scan(&d.ID, &d.OutletID, &d.Code, &d.Name, &scope, &method, &d.ValueBps, &d.ValuePaise,
		&d.MaxDiscountPaise, &d.RequiredPermission, &d.RequiresReason, &d.IsActive,
		&d.EffectiveFrom, &d.EffectiveTo, &d.ConfigVersion)
	if errors.Is(err, pgx.ErrNoRows) {
		return contracts.DiscountDefinition{}, fmt.Errorf("%w: discount definition %s", httpx.ErrNotFound, id)
	}
	if err != nil {
		return contracts.DiscountDefinition{}, fmt.Errorf("compliance: getting discount_definition: %w", err)
	}
	d.Scope = contracts.DiscountScope(scope)
	d.Method = contracts.DiscountMethod(method)
	d.SchemaVersion = 1
	return d, nil
}

func (r *pgRepository) DiscountDefinitionsSince(ctx context.Context, outletID string, sinceVersion int) ([]contracts.DiscountDefinition, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT id, outlet_id, code, name, scope, method, value_bps, value_paise,
		        max_discount_paise, required_permission, requires_reason, is_active,
		        effective_from, effective_to, config_version
		 FROM discount_definition WHERE outlet_id = $1 AND config_version > $2 ORDER BY code`,
		outletID, sinceVersion,
	)
	if err != nil {
		return nil, fmt.Errorf("compliance: listing discount_definition: %w", err)
	}
	defer rows.Close()

	var out []contracts.DiscountDefinition
	for rows.Next() {
		var d contracts.DiscountDefinition
		var scope, method string
		if err := rows.Scan(&d.ID, &d.OutletID, &d.Code, &d.Name, &scope, &method, &d.ValueBps, &d.ValuePaise,
			&d.MaxDiscountPaise, &d.RequiredPermission, &d.RequiresReason, &d.IsActive,
			&d.EffectiveFrom, &d.EffectiveTo, &d.ConfigVersion); err != nil {
			return nil, fmt.Errorf("compliance: scanning discount_definition: %w", err)
		}
		d.Scope = contracts.DiscountScope(scope)
		d.Method = contracts.DiscountMethod(method)
		d.SchemaVersion = 1
		out = append(out, d)
	}
	return out, rows.Err()
}

func (r *pgRepository) SetDiscountDefinitionActive(ctx context.Context, tx pgx.Tx, id string, isActive bool, configVersion int) error {
	tag, err := tx.Exec(ctx,
		`UPDATE discount_definition SET is_active = $1, config_version = $2 WHERE id = $3`,
		isActive, configVersion, id,
	)
	if err != nil {
		return fmt.Errorf("compliance: updating discount_definition active state: %w", err)
	}
	if tag.RowsAffected() == 0 {
		return fmt.Errorf("%w: discount definition %s", httpx.ErrNotFound, id)
	}
	return nil
}

// --- outlet_fiscal_profile ---------------------------------------------------

func (r *pgRepository) InsertFiscalProfile(ctx context.Context, tx pgx.Tx, fp contracts.OutletFiscalProfile) error {
	_, err := tx.Exec(ctx,
		`INSERT INTO outlet_fiscal_profile (
			id, outlet_id, legal_name, trade_name, address_line1, address_line2, city,
			state_code, state_name, pincode, gstin, fssai_number, invoice_footer_text,
			effective_from, config_version
		 ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)`,
		fp.ID, fp.OutletID, fp.LegalName, fp.TradeName, fp.AddressLine1, fp.AddressLine2, fp.City,
		fp.StateCode, fp.StateName, fp.Pincode, fp.GSTIN, fp.FSSAINumber, fp.InvoiceFooterText,
		fp.EffectiveFrom, fp.ConfigVersion,
	)
	if err != nil {
		if storage.IsUniqueViolation(err) {
			return fmt.Errorf("%w: outlet already has a fiscal profile effective at this instant", httpx.ErrConflict)
		}
		return fmt.Errorf("compliance: inserting outlet_fiscal_profile: %w", err)
	}
	return nil
}

func (r *pgRepository) CurrentFiscalProfile(ctx context.Context, outletID string) (*contracts.OutletFiscalProfile, error) {
	var fp contracts.OutletFiscalProfile
	err := r.pool.QueryRow(ctx,
		`SELECT id, outlet_id, legal_name, trade_name, address_line1, address_line2, city,
		        state_code, state_name, pincode, gstin, fssai_number, invoice_footer_text,
		        effective_from, config_version
		 FROM outlet_fiscal_profile
		 WHERE outlet_id = $1 AND effective_from <= now()
		 ORDER BY effective_from DESC LIMIT 1`,
		outletID,
	).Scan(&fp.ID, &fp.OutletID, &fp.LegalName, &fp.TradeName, &fp.AddressLine1, &fp.AddressLine2, &fp.City,
		&fp.StateCode, &fp.StateName, &fp.Pincode, &fp.GSTIN, &fp.FSSAINumber, &fp.InvoiceFooterText,
		&fp.EffectiveFrom, &fp.ConfigVersion)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, nil
	}
	if err != nil {
		return nil, fmt.Errorf("compliance: getting outlet_fiscal_profile: %w", err)
	}
	fp.SchemaVersion = 1
	return &fp, nil
}
