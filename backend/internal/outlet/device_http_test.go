package outlet

import (
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestEnrollDeviceHTTP_ReturnsTokenOnce(t *testing.T) {
	_, _, svc := newDeviceTestFixture()
	h := NewDeviceHandler(svc)

	body, _ := json.Marshal(enrollDeviceRequest{OutletID: "outlet-a", Kind: "POS", Name: "POS-1", Label: "install"})
	req := httptest.NewRequest(http.MethodPost, "/devices/enroll", bytes.NewReader(body))
	req = req.WithContext(WithPrincipal(req.Context(), Principal{TenantID: "tenant-a", UserID: "user-1"}))
	rec := httptest.NewRecorder()

	h.enroll(rec, req)

	if rec.Code != http.StatusCreated {
		t.Fatalf("expected 201, got %d: %s", rec.Code, rec.Body.String())
	}
	var resp enrolledDeviceResponse
	if err := json.Unmarshal(rec.Body.Bytes(), &resp); err != nil {
		t.Fatalf("decoding response: %v", err)
	}
	if resp.Token == "" {
		t.Fatal("expected a non-empty token in the enroll response")
	}
}

func TestEnrollDeviceHTTP_UnauthorizedWithoutPrincipal(t *testing.T) {
	_, _, svc := newDeviceTestFixture()
	h := NewDeviceHandler(svc)

	req := httptest.NewRequest(http.MethodPost, "/devices/enroll", bytes.NewReader([]byte(`{}`)))
	rec := httptest.NewRecorder()

	h.enroll(rec, req)

	if rec.Code != http.StatusUnauthorized {
		t.Fatalf("expected 401, got %d", rec.Code)
	}
}

// TestDeviceAuthenticate_RejectsMissingOrInvalidToken is the falsifying test
// for the credential-verification middleware itself: prove it rejects a
// request an unauthenticated caller can actually send, not merely that a
// happy path accepts a good one.
func TestDeviceAuthenticate_RejectsMissingOrInvalidToken(t *testing.T) {
	_, _, svc := newDeviceTestFixture()
	mw := DeviceAuthenticate(svc)

	called := false
	next := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { called = true })

	cases := []struct {
		name   string
		header string
	}{
		{"no header", ""},
		{"empty bearer", "Bearer "},
		{"garbage token", "Bearer not-a-real-token"},
		{"well-formed but unknown credential id", "Bearer 00000000-0000-7000-8000-000000000000.secret"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			called = false
			req := httptest.NewRequest(http.MethodGet, "/sync/config", nil)
			if tc.header != "" {
				req.Header.Set("Authorization", tc.header)
			}
			rec := httptest.NewRecorder()
			mw(next).ServeHTTP(rec, req)

			if rec.Code != http.StatusUnauthorized {
				t.Fatalf("%s: expected 401, got %d", tc.name, rec.Code)
			}
			if called {
				t.Fatalf("%s: downstream handler must not run on a rejected credential", tc.name)
			}
		})
	}
}

func TestDeviceAuthenticate_AcceptsValidTokenAndResolvesPrincipal(t *testing.T) {
	_, _, svc := newDeviceTestFixture()
	enrolled, err := svc.EnrollDevice(context.Background(), Principal{TenantID: "tenant-a"}, "outlet-a", DeviceKindPOS, "POS-1", "", nil)
	if err != nil {
		t.Fatalf("EnrollDevice: %v", err)
	}

	mw := DeviceAuthenticate(svc)
	var gotPrincipal DevicePrincipal
	var gotOK bool
	next := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotPrincipal, gotOK = DevicePrincipalFromContext(r.Context())
		w.WriteHeader(http.StatusOK)
	})

	req := httptest.NewRequest(http.MethodGet, "/sync/config", nil)
	req.Header.Set("Authorization", "Bearer "+enrolled.Token)
	rec := httptest.NewRecorder()
	mw(next).ServeHTTP(rec, req)

	if rec.Code != http.StatusOK {
		t.Fatalf("expected 200, got %d", rec.Code)
	}
	if !gotOK {
		t.Fatal("expected a device principal in the downstream context")
	}
	if gotPrincipal.TenantID != "tenant-a" || gotPrincipal.OutletID != "outlet-a" || gotPrincipal.DeviceID != enrolled.Device.ID {
		t.Fatalf("unexpected device principal: %+v", gotPrincipal)
	}
}
