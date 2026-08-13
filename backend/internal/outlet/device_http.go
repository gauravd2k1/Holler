package outlet

import (
	"net/http"

	"github.com/go-chi/chi/v5"
	"github.com/holler/backend/internal/platform/httpx"
)

// DeviceHandler wires the device enrollment HTTP surface. Every route here
// requires an authenticated, privileged human principal (outlet.manage) —
// this is the management side, not the device's own credential path.
// Contains no business logic (CLAUDE.md §Coding rules).
type DeviceHandler struct {
	svc *DeviceService
}

func NewDeviceHandler(svc *DeviceService) *DeviceHandler {
	return &DeviceHandler{svc: svc}
}

// Mount registers POST /devices/enroll, POST /devices/{deviceId}/credentials/rotate
// and POST /devices/{deviceId}/credentials/revoke. These are not yet part of
// packages/contracts/openapi/openapi.yaml (see this track's final report) —
// the shapes below are additive and were designed against ADR-017, not
// against a frozen OpenAPI path, because none exists yet for enrollment.
func (h *DeviceHandler) Mount(r chi.Router) {
	r.Post("/devices/enroll", h.enroll)
	r.Post("/devices/{deviceId}/credentials/rotate", h.rotate)
	r.Post("/devices/{deviceId}/credentials/revoke", h.revoke)
}

// enrolledDeviceResponse carries the plaintext token EXACTLY ONCE. No other
// response type in this package (or anywhere in backend/internal/outlet)
// ever includes a token field.
type enrolledDeviceResponse struct {
	DeviceID     string `json:"device_id"`
	OutletID     string `json:"outlet_id"`
	Kind         string `json:"kind"`
	Name         string `json:"name"`
	CredentialID string `json:"credential_id"`
	Token        string `json:"token"`
}

func toEnrolledResponse(e EnrolledDevice) enrolledDeviceResponse {
	return enrolledDeviceResponse{
		DeviceID:     e.Device.ID,
		OutletID:     e.Device.OutletID,
		Kind:         string(e.Device.Kind),
		Name:         e.Device.Name,
		CredentialID: e.CredentialID,
		Token:        e.Token,
	}
}

type enrollDeviceRequest struct {
	OutletID string `json:"outlet_id"`
	Kind     string `json:"kind"`
	Name     string `json:"name"`
	Label    string `json:"label"`
}

func (h *DeviceHandler) enroll(w http.ResponseWriter, r *http.Request) {
	principal, ok := PrincipalFromContext(r.Context())
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	var req enrollDeviceRequest
	if err := httpx.DecodeJSON(r, &req); err != nil {
		httpx.Error(w, err)
		return
	}

	actor := principal.UserID
	result, err := h.svc.EnrollDevice(r.Context(), principal, req.OutletID, DeviceKind(req.Kind), req.Name, req.Label, &actor)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusCreated, toEnrolledResponse(result))
}

type rotateCredentialRequest struct {
	Label string `json:"label"`
}

func (h *DeviceHandler) rotate(w http.ResponseWriter, r *http.Request) {
	principal, ok := PrincipalFromContext(r.Context())
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	deviceID := chi.URLParam(r, "deviceId")

	var req rotateCredentialRequest
	if r.ContentLength > 0 {
		if err := httpx.DecodeJSON(r, &req); err != nil {
			httpx.Error(w, err)
			return
		}
	}

	actor := principal.UserID
	result, err := h.svc.RotateCredential(r.Context(), principal, deviceID, req.Label, &actor)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusCreated, toEnrolledResponse(result))
}

func (h *DeviceHandler) revoke(w http.ResponseWriter, r *http.Request) {
	principal, ok := PrincipalFromContext(r.Context())
	if !ok {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	deviceID := chi.URLParam(r, "deviceId")

	actor := principal.UserID
	if err := h.svc.RevokeCredential(r.Context(), principal, deviceID, &actor); err != nil {
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusNoContent, nil)
}
