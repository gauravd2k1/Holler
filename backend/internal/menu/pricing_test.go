package menu

import (
	"errors"
	"testing"

	"github.com/holler/backend/internal/platform/httpx"
)

func TestComposePrice(t *testing.T) {
	cases := []struct {
		name             string
		basePricePaise   int64
		variantDelta     int64
		modifierDeltas   []int64
		wantTotalPaise   int64
	}{
		{"base only", 25000, 0, nil, 25000},
		{"base plus variant", 25000, 5000, nil, 30000},
		{"base plus modifiers", 25000, 0, []int64{2000, 3000}, 30000},
		{"base plus variant plus modifiers", 41000, 1000, []int64{500, -500}, 42000},
		{"negative modifier delta (discount option)", 20000, 0, []int64{-2000}, 18000},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got := ComposePrice(tc.basePricePaise, tc.variantDelta, tc.modifierDeltas)
			if got != tc.wantTotalPaise {
				t.Fatalf("ComposePrice() = %d paise, want %d paise", got, tc.wantTotalPaise)
			}
		})
	}
}

func TestValidateModifierGroupSelection(t *testing.T) {
	t.Run("within bounds", func(t *testing.T) {
		if err := ValidateModifierGroupSelection("Toppings", 1, 3, 2); err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
	})

	t.Run("below minimum", func(t *testing.T) {
		err := ValidateModifierGroupSelection("Size", 1, 1, 0)
		if !errors.Is(err, httpx.ErrInvalidInput) {
			t.Fatalf("expected ErrInvalidInput, got %v", err)
		}
	})

	t.Run("above maximum", func(t *testing.T) {
		err := ValidateModifierGroupSelection("Crust", 0, 1, 2)
		if !errors.Is(err, httpx.ErrInvalidInput) {
			t.Fatalf("expected ErrInvalidInput, got %v", err)
		}
	})

	t.Run("max zero means unbounded", func(t *testing.T) {
		if err := ValidateModifierGroupSelection("Toppings", 0, 0, 10); err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
	})
}
