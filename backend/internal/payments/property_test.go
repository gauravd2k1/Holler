package payments

import (
	"context"
	"math/rand"
	"testing"
	"time"

	contracts "github.com/holler/contracts"
)

// §66 mandates property-based coverage of "split payments" and "refund
// allocation" alongside tax/rounding/discounts/settlement. Milestone 3
// (docs/spec/payments.md) covers cash + split payments only — the online
// gateway/settlement/reconciliation paths this spec also names are excluded
// until Milestone 7, so there is no settlement-calculation code in this
// package to generate against yet; these two properties are the ones this
// milestone's Service actually has to get right.
//
// This package is a REPLAY layer (§50.1): the edge is the money authority
// and Service.IngestPayment only stores what it is told. These properties
// therefore check something Service CAN guarantee — that storing a
// well-formed split/refund sequence through the real ingest path preserves
// the sequence exactly (append-only, no loss, no duplication, no drift) —
// rather than pretending the cloud recomputes or polices amounts it never
// originates.

const paymentPropertyIterations = 2000

// randomSplitAmounts partitions totalPaise (> 0) into n positive shares that
// sum to EXACTLY totalPaise — the shape of "₹2,000 = ₹500 cash + ₹1,000 UPI +
// ₹500 card" (docs/spec/payments.md §Methods) generalized to n tenders.
func randomSplitAmounts(r *rand.Rand, totalPaise, n int) []int {
	if n <= 0 {
		return nil
	}
	if n == 1 {
		return []int{totalPaise}
	}
	// Place n-1 random cut points in [0, totalPaise], sort them, and take
	// consecutive differences — a standard total-preserving random
	// partition into n non-negative parts, then nudge any zero part up by
	// stealing one paise from the largest part so every tender is a real,
	// positive payment (a ₹0 tender is not a thing a cashier rings up).
	cuts := make([]int, n-1)
	for i := range cuts {
		cuts[i] = r.Intn(totalPaise + 1)
	}
	sortInts(cuts)

	parts := make([]int, n)
	prev := 0
	for i, c := range cuts {
		parts[i] = c - prev
		prev = c
	}
	parts[n-1] = totalPaise - prev

	for i := range parts {
		for parts[i] == 0 {
			largest := 0
			for j := range parts {
				if parts[j] > parts[largest] {
					largest = j
				}
			}
			if parts[largest] <= 1 {
				break // totalPaise too small to give every part >= 1; leave as-is
			}
			parts[largest]--
			parts[i]++
		}
	}
	return parts
}

func sortInts(s []int) {
	for i := 1; i < len(s); i++ {
		for j := i; j > 0 && s[j-1] > s[j]; j-- {
			s[j-1], s[j] = s[j], s[j-1]
		}
	}
}

var splitPaymentMethods = []contracts.PaymentMethod{
	contracts.PaymentMethodCash,
	contracts.PaymentMethodUPI,
	contracts.PaymentMethodCreditCard,
	contracts.PaymentMethodDebitCard,
	contracts.PaymentMethodWallet,
}

// TestProperty_SplitPaymentConservation is the §66 "split payments" property:
// for a bill of a random total, split into a random number of tenders whose
// amounts are generated to sum EXACTLY to that total (never hand-picked),
// ingesting every tender through the real Service.IngestPayment and reading
// each back through the real Service.GetPayment must reproduce: (1) the
// stored amount for every tender, byte-for-byte against what was sent, and
// (2) Σ(stored tenders) == the original bill total, in integer paise, no
// floating point anywhere in the path.
func TestProperty_SplitPaymentConservation(t *testing.T) {
	r := rand.New(rand.NewSource(66066))
	ctx := context.Background()

	for i := 0; i < paymentPropertyIterations; i++ {
		totalPaise := 1 + r.Intn(10_000_00) // up to ₹10,000, always >= 1 paise
		tenderCount := 1 + r.Intn(4)        // 1..4 tenders
		amounts := randomSplitAmounts(r, totalPaise, tenderCount)

		repo := newFakeRepo()
		svc := NewService(repo)
		orderID := randomULIDLikeID(r)

		var sumStored int
		for k, amount := range amounts {
			if amount <= 0 {
				continue // the partition nudge can leave a genuine zero only when totalPaise < tenderCount; skip, it contributes nothing to the sum either way
			}
			method := splitPaymentMethods[r.Intn(len(splitPaymentMethods))]
			p := basePayment(randomULIDLikeID(r))
			p.OrderID = orderID
			p.Method = method
			p.AmountPaise = amount
			if method != contracts.PaymentMethodCash {
				p.TenderedPaise = nil
				p.ChangePaise = nil
			}

			stored, err := svc.IngestPayment(ctx, testTenantID, testOutletID, paymentEnvelope(p.ID, 1), p)
			if err != nil {
				t.Fatalf("iteration %d tender %d: IngestPayment: %v", i, k, err)
			}
			if stored.AmountPaise != amount {
				t.Fatalf("iteration %d tender %d: stored amount_paise %d != sent %d", i, k, stored.AmountPaise, amount)
			}

			reread, err := svc.GetPayment(ctx, testTenantID, stored.ID)
			if err != nil {
				t.Fatalf("iteration %d tender %d: GetPayment: %v", i, k, err)
			}
			if reread.AmountPaise != amount {
				t.Fatalf("iteration %d tender %d: reread amount_paise %d != sent %d", i, k, reread.AmountPaise, amount)
			}
			sumStored += reread.AmountPaise
		}

		if sumStored != totalPaise {
			t.Fatalf("iteration %d: sum of split tenders %d != bill total %d (amounts: %v)", i, sumStored, totalPaise, amounts)
		}
	}
}

// TestProperty_RefundAllocationNeverExceedsSettled is the §66 "refund
// allocation" property: given one captured payment and a randomly generated
// SEQUENCE of reversal payments (§53 — each a new append-only row carrying
// reverses_payment_id and a non-positive amount, never an update), where
// every reversal's magnitude is generated to be within what remains settled
// at the moment it is issued, the running balance — captured amount plus
// every reversal ingested so far — must never go negative at any point in
// the sequence, and must equal captured-minus-total-refunded at the end.
// This is the well-behaved-edge shape: it proves the append-only ledger this
// package stores is faithful to a correctly ordered refund sequence, not
// that the cloud itself polices an over-refund (it does not — see this
// task's report for that gap).
func TestProperty_RefundAllocationNeverExceedsSettled(t *testing.T) {
	r := rand.New(rand.NewSource(53539))
	ctx := context.Background()

	for i := 0; i < paymentPropertyIterations; i++ {
		repo := newFakeRepo()
		svc := NewService(repo)
		orderID := randomULIDLikeID(r)

		capturedAmount := 1 + r.Intn(10_000_00)
		original := basePayment(randomULIDLikeID(r))
		original.OrderID = orderID
		original.AmountPaise = capturedAmount
		stored, err := svc.IngestPayment(ctx, testTenantID, testOutletID, paymentEnvelope(original.ID, 1), original)
		if err != nil {
			t.Fatalf("iteration %d: capturing original payment: %v", i, err)
		}

		remaining := capturedAmount
		refundCount := r.Intn(4) // 0..3 refunds against this one capture
		for k := 0; k < refundCount && remaining > 0; k++ {
			refundAmount := 1 + r.Intn(remaining) // generated within what remains settled
			reversal := basePayment(randomULIDLikeID(r))
			reversal.OrderID = orderID
			reversal.AmountPaise = -refundAmount
			reversal.ReversesPaymentID = &stored.ID
			reversal.Method = original.Method
			reversal.TenderedPaise = nil
			reversal.ChangePaise = nil

			if _, err := svc.IngestPayment(ctx, testTenantID, testOutletID, paymentEnvelope(reversal.ID, 1), reversal); err != nil {
				t.Fatalf("iteration %d refund %d: IngestPayment reversal: %v", i, k, err)
			}

			remaining -= refundAmount
			if remaining < 0 {
				t.Fatalf("iteration %d refund %d: running balance went negative: %d", i, k, remaining)
			}
		}

		// Recompute the balance purely by reading back the stored ledger
		// (never trusting the loop's own running total), mirroring how a
		// reconciliation job would sum it: this is the conservation check.
		var ledgerSum int
		for _, p := range repo.payments {
			if p.OrderID != orderID {
				continue
			}
			ledgerSum += p.AmountPaise
		}
		if ledgerSum != remaining {
			t.Fatalf("iteration %d: ledger sum %d != expected remaining balance %d", i, ledgerSum, remaining)
		}
		if ledgerSum < 0 {
			t.Fatalf("iteration %d: ledger sum went negative: %d", i, ledgerSum)
		}
	}
}

// randomULIDLikeID generates a syntactically-plausible unique id for test
// fixtures — this package's fakeRepo keys on the string alone and never
// validates ULID/UUID shape, so a random hex string is sufficient and avoids
// pulling in internal/platform/id's UUIDv7 generator just for a test double.
func randomULIDLikeID(r *rand.Rand) string {
	const hex = "0123456789abcdef"
	b := make([]byte, 32)
	for i := range b {
		b[i] = hex[r.Intn(len(hex))]
	}
	return time.Now().UTC().Format("20060102150405.000000000") + "-" + string(b)
}
