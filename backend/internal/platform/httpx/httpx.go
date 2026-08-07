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
