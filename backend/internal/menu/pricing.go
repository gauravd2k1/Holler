package menu

import (
	"fmt"

	"github.com/holler/backend/internal/platform/httpx"
)

// ComposePrice returns the price of an item in integer paise: base price,
// plus the selected variant's delta (if any), plus every selected
// modifier's delta. All inputs and the result are integers — no float ever
// touches money in this package.
func ComposePrice(basePricePaise int64, variantDeltaPaise int64, modifierDeltasPaise []int64) int64 {
	total := basePricePaise + variantDeltaPaise
	for _, d := range modifierDeltasPaise {
		total += d
	}
	return total
}

// ValidateModifierGroupSelection checks that the number of selected options
// within a single modifier group falls within [minSelection, maxSelection].
func ValidateModifierGroupSelection(groupName string, minSelection, maxSelection, selectedCount int) error {
	if selectedCount < minSelection {
		return fmt.Errorf("%w: group %q requires at least %d selection(s), got %d", httpx.ErrInvalidInput, groupName, minSelection, selectedCount)
	}
	if maxSelection > 0 && selectedCount > maxSelection {
		return fmt.Errorf("%w: group %q allows at most %d selection(s), got %d", httpx.ErrInvalidInput, groupName, maxSelection, selectedCount)
	}
	return nil
}
