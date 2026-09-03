// Package httpx holds the HTTP plumbing shared by every bounded context:
// JSON encoding, a single error envelope, and request-scoped logging. HTTP
// handlers use these; they never contain database or business logic
// (CLAUDE.md §Coding rules).
package httpx

import (
	"encoding/json"
	"errors"
	"log/slog"
	"net/http"
)

// ErrorBody is the one error shape every endpoint returns.
type ErrorBody struct {
	Code    string `json:"code"`    // machine-readable, e.g. "not_found", "forbidden"
	Message string `json:"message"` // human-readable, never contains secrets or SQL
}

// Domain errors that every context maps its failures onto, so the HTTP layer
// stays free of per-module status-code tables.
var (
	ErrNotFound     = errors.New("not found")
	ErrInvalidInput = errors.New("invalid input")
	ErrConflict     = errors.New("conflict")
	ErrUnauthorized = errors.New("unauthorized")
	ErrForbidden    = errors.New("forbidden")

	// ErrMissingReference is a request naming a row that does not exist —
	// a foreign key that does not resolve (SQLSTATE 23503, mapped in
	// internal/platform/storage).
	//
	// It is 422 and NOT 409. A conflict says "the state disagrees, a retry
	// may resolve it"; a menu_item the cloud has never held is not a
	// conflict and no retry will ever resolve it. The edge classifies every
	// 4xx as permanent either way, so this distinction buys nothing on the
	// wire and everything in the record: the operator reading it is told
	// which of the two situations they are in.
	ErrMissingReference = errors.New("missing reference")
)

func JSON(w http.ResponseWriter, status int, body any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	if body == nil {
		return
	}
	if err := json.NewEncoder(w).Encode(body); err != nil {
		slog.Error("httpx: encoding response", "error", err)
	}
}

// Error maps a domain error to a status code and writes the envelope. The
// underlying error text is logged, never returned, so internal detail cannot
// leak to a client.
func Error(w http.ResponseWriter, err error) {
	status, code := http.StatusInternalServerError, "internal_error"
	message := "an unexpected error occurred"

	switch {
	case errors.Is(err, ErrNotFound):
		status, code, message = http.StatusNotFound, "not_found", "resource not found"
	case errors.Is(err, ErrInvalidInput):
		status, code, message = http.StatusBadRequest, "invalid_input", err.Error()
	case errors.Is(err, ErrMissingReference):
		// 422 with a stable machine-readable code, because the EDGE reads
		// this: it is the only thing distinguishing "your data is wrong and
		// always will be" from "come back later". Reported as 500 before
		// M6 A1, which made the edge retry a permanently-broken row forever
		// and strand every row behind it.
		status, code, message = http.StatusUnprocessableEntity, "missing_reference", err.Error()
	case errors.Is(err, ErrConflict):
		status, code, message = http.StatusConflict, "conflict", err.Error()
	case errors.Is(err, ErrUnauthorized):
		status, code, message = http.StatusUnauthorized, "unauthorized", "authentication required"
	case errors.Is(err, ErrForbidden):
		status, code, message = http.StatusForbidden, "forbidden", "insufficient permission"
	default:
		slog.Error("httpx: unhandled error", "error", err)
	}

	JSON(w, status, ErrorBody{Code: code, Message: message})
}

// DecodeJSON reads a request body into dst, rejecting unknown fields so a
// typo'd field is a 400 rather than a silently ignored value.
func DecodeJSON(r *http.Request, dst any) error {
	dec := json.NewDecoder(r.Body)
	dec.DisallowUnknownFields()
	if err := dec.Decode(dst); err != nil {
		return ErrInvalidInput
	}
	return nil
}
