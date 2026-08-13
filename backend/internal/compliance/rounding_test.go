package compliance

import "testing"

func TestRoundHalfUpDiv(t *testing.T) {
	cases := []struct {
		name   string
		num    int64
		den    int64
		expect int64
	}{
		{"exact", 100, 10, 10},
		{"below half rounds down", 104, 10, 10},
		{"exact half rounds up", 105, 10, 11}, // the ₹x.x5 case ADR-016 names explicitly
		{"above half rounds up", 106, 10, 11},
		{"zero numerator", 0, 10, 0},
		{"large bps division", 12550*250 + 1, 10000, 314}, // 2.5% of 125.50 plus a hair
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			got := roundHalfUpDiv(c.num, c.den)
			if got != c.expect {
				t.Fatalf("roundHalfUpDiv(%d, %d) = %d, want %d", c.num, c.den, got, c.expect)
			}
		})
	}
}

func TestRoundToNearestRupee(t *testing.T) {
	cases := []struct {
		amount int64
		expect int64
	}{
		{0, 0},
		{1, 0},
		{49, 0},
		{50, 100}, // exact half rounds up
		{51, 100},
		{149, 100},
		{150, 200},
		{12550, 12600}, // ₹125.50 rounds up to ₹126.00
	}
	for _, c := range cases {
		got := roundToNearestRupee(c.amount)
		if got != c.expect {
			t.Fatalf("roundToNearestRupee(%d) = %d, want %d", c.amount, got, c.expect)
		}
		if got%paiseToRupeeDenominator != 0 {
			t.Fatalf("roundToNearestRupee(%d) = %d is not a whole rupee", c.amount, got)
		}
	}
}

func TestLargestRemainderSplit_SumsExactly(t *testing.T) {
	cases := []struct {
		name    string
		total   int64
		weights []int64
	}{
		{"even split", 100, []int64{250, 250, 0, 0}},
		{"cess stacked on gst", 137, []int64{250, 250, 0, 500}},
		{"single weight", 7, []int64{1800, 0, 0, 0}},
		{"all zero weights", 5, []int64{0, 0, 0, 0}},
		{"zero total", 0, []int64{250, 250, 0, 500}},
		{"one paise many components", 1, []int64{250, 250, 0, 500}},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			shares := largestRemainderSplit(c.total, c.weights)
			var sum int64
			for _, s := range shares {
				if s < 0 {
					t.Fatalf("negative share %d in %v", s, shares)
				}
				sum += s
			}
			if sum != c.total {
				t.Fatalf("largestRemainderSplit(%d, %v) = %v, sums to %d, want %d",
					c.total, c.weights, shares, sum, c.total)
			}
		})
	}
}
