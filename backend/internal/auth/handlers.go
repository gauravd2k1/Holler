package auth

import (
	"net"
	"net/http"

	"github.com/go-chi/chi/v5"
	"github.com/holler/backend/internal/platform/httpx"
)

// Handlers wires the auth HTTP surface: POST /auth/login, POST
// /auth/refresh, POST /auth/logout, GET /users, POST /users, PUT
// /users/{id}/roles, GET /roles (packages/contracts/openapi/openapi.yaml).
type Handlers struct {
	service *Service
	tokens  *TokenSigner
}

func NewHandlers(service *Service, tokens *TokenSigner) *Handlers {
	return &Handlers{service: service, tokens: tokens}
}

// Mount attaches every auth route onto r. /users and /roles routes require
// an authenticated principal with user.manage.
func (h *Handlers) Mount(r chi.Router) {
	r.Post("/auth/login", h.login)
	r.Post("/auth/refresh", h.refresh)
	r.Post("/auth/logout", h.logout)

	r.Group(func(r chi.Router) {
		r.Use(Authenticate(h.tokens))
		r.With(RequirePermission(PermissionUserManage)).Get("/users", h.listUsers)
		r.With(RequirePermission(PermissionUserManage)).Post("/users", h.createUser)
		r.With(RequirePermission(PermissionUserManage)).Put("/users/{id}/roles", h.setUserRoles)
		r.With(RequirePermission(PermissionUserManage)).Post("/users/{id}/password", h.changePassword)
		r.With(RequirePermission(PermissionUserManage)).Post("/users/{id}/pin", h.changePin)
		r.Get("/roles", h.listRoles)
	})
}

type loginRequest struct {
	Email    string `json:"email"`
	Password string `json:"password"`
	OutletID string `json:"outlet_id"`
}

type sessionResponse struct {
	AccessToken  string                 `json:"access_token"`
	RefreshToken string                 `json:"refresh_token"`
	Principal    AuthenticatedPrincipal `json:"principal"`
}

// resolveTenantID is the ONE place in this package that resolves the tenant
// of an unauthenticated request (ADR-012 "Host-based tenant resolution,
// with X-Tenant-ID as a time-boxed interim"). Every other request in this
// package (post-login) gets its tenant from the resolved
// AuthenticatedPrincipal, never from a header — POST /auth/login is the one
// endpoint with no principal yet, so it is the one place this seam is
// needed.
//
// Confined here so that swapping the client-supplied header for TLS-SNI /
// host-based resolution — required before Holler serves more than one
// tenant in production — is a single-function change. When that swap
// happens, ADR-012 requires X-Tenant-ID to be *rejected*, not silently
// ignored; that rejection also belongs only here.
func resolveTenantID(r *http.Request) string {
	return r.Header.Get("X-Tenant-ID")
}

// clientIP extracts the caller's address for the ADR-012 login rate limit
// key. It reads RemoteAddr, which in this deployment is set by the
// connecting peer (no trusted reverse proxy sits in front of Milestone 1's
// local stack) — X-Forwarded-For is deliberately not trusted here, since an
// unauthenticated caller could forge it to reset their own rate-limit
// bucket, defeating the IP component ADR-012 relies on.
func clientIP(r *http.Request) string {
	host, _, err := net.SplitHostPort(r.RemoteAddr)
	if err != nil {
		return r.RemoteAddr
	}
	return host
}

func (h *Handlers) login(w http.ResponseWriter, r *http.Request) {
	var req loginRequest
	if err := httpx.DecodeJSON(r, &req); err != nil {
		httpx.Error(w, err)
		return
	}
	tenantID := resolveTenantID(r)
	if tenantID == "" || req.Email == "" || req.Password == "" || req.OutletID == "" {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}

	result, err := h.service.Login(r.Context(), clientIP(r), tenantID, req.Email, req.Password, req.OutletID)
	if err != nil {
		// ErrRateLimited maps onto the identical unauthorized response as a
		// bad credential: rate-limit rejection must not be distinguishable
		// from "wrong password", which would otherwise leak that throttling
		// is specifically account-related (ADR-012).
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	httpx.JSON(w, http.StatusOK, sessionResponse{
		AccessToken:  result.AccessToken,
		RefreshToken: result.RefreshToken,
		Principal:    result.Principal,
	})
}

type refreshRequest struct {
	RefreshToken string `json:"refresh_token"`
}

func (h *Handlers) refresh(w http.ResponseWriter, r *http.Request) {
	var req refreshRequest
	if err := httpx.DecodeJSON(r, &req); err != nil {
		httpx.Error(w, err)
		return
	}
	if req.RefreshToken == "" {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}
	result, err := h.service.Refresh(r.Context(), req.RefreshToken)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusOK, sessionResponse{
		AccessToken:  result.AccessToken,
		RefreshToken: result.RefreshToken,
		Principal:    result.Principal,
	})
}

func (h *Handlers) logout(w http.ResponseWriter, r *http.Request) {
	var req refreshRequest
	if err := httpx.DecodeJSON(r, &req); err != nil {
		httpx.Error(w, err)
		return
	}
	if req.RefreshToken != "" {
		h.service.Logout(r.Context(), req.RefreshToken)
	}
	httpx.JSON(w, http.StatusNoContent, nil)
}

func (h *Handlers) listUsers(w http.ResponseWriter, r *http.Request) {
	principal, _ := PrincipalFromContext(r.Context())
	users, err := h.service.ListUsers(r.Context(), principal.TenantID)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	// users is []User = []contracts.AppUser, which never carries a hash field
	// (packages/contracts/go/identity.go) — safe to marshal directly.
	if users == nil {
		users = []User{}
	}
	httpx.JSON(w, http.StatusOK, users)
}

type createUserRequest struct {
	ID       string `json:"id"`
	Email    string `json:"email"`
	FullName string `json:"full_name"`
	Password string `json:"password"`
}

func (h *Handlers) createUser(w http.ResponseWriter, r *http.Request) {
	principal, _ := PrincipalFromContext(r.Context())
	var req createUserRequest
	if err := httpx.DecodeJSON(r, &req); err != nil {
		httpx.Error(w, err)
		return
	}
	if req.ID == "" {
		httpx.Error(w, httpx.ErrInvalidInput)
		return
	}

	actor := principal.UserID
	user, err := h.service.CreateUser(r.Context(), principal.TenantID, req.ID, req.Email, req.FullName, req.Password, &actor, nil)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusCreated, user)
}

type roleAssignmentRequest struct {
	ID       string  `json:"id"`
	RoleID   string  `json:"role_id"`
	OutletID *string `json:"outlet_id"`
}

func (h *Handlers) setUserRoles(w http.ResponseWriter, r *http.Request) {
	principal, _ := PrincipalFromContext(r.Context())
	userID := chi.URLParam(r, "id")

	var req []roleAssignmentRequest
	if err := httpx.DecodeJSON(r, &req); err != nil {
		httpx.Error(w, err)
		return
	}

	inputs := make([]RoleAssignmentInput, 0, len(req))
	for _, a := range req {
		inputs = append(inputs, RoleAssignmentInput{ID: a.ID, RoleID: a.RoleID, OutletID: a.OutletID})
	}

	actor := principal.UserID
	user, err := h.service.SetUserRoles(r.Context(), principal.TenantID, userID, inputs, &actor, nil)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusOK, user)
}

type changePasswordRequest struct {
	Password string `json:"password"`
}

// changePassword mints a new password_hash for /users/{id} and bumps
// config_version (ADR-017 §4). Mounted alongside /users/{id}/roles, same
// permission — user.manage is the same authority that can already assign
// roles, which is at least as sensitive.
func (h *Handlers) changePassword(w http.ResponseWriter, r *http.Request) {
	principal, _ := PrincipalFromContext(r.Context())
	userID := chi.URLParam(r, "id")

	var req changePasswordRequest
	if err := httpx.DecodeJSON(r, &req); err != nil {
		httpx.Error(w, err)
		return
	}

	actor := principal.UserID
	user, err := h.service.ChangePassword(r.Context(), principal.TenantID, userID, req.Password, &actor, nil)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusOK, user)
}

type changePinRequest struct {
	Pin string `json:"pin"`
}

func (h *Handlers) changePin(w http.ResponseWriter, r *http.Request) {
	principal, _ := PrincipalFromContext(r.Context())
	userID := chi.URLParam(r, "id")

	var req changePinRequest
	if err := httpx.DecodeJSON(r, &req); err != nil {
		httpx.Error(w, err)
		return
	}

	actor := principal.UserID
	user, err := h.service.ChangePin(r.Context(), principal.TenantID, userID, req.Pin, &actor, nil)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusOK, user)
}

func (h *Handlers) listRoles(w http.ResponseWriter, r *http.Request) {
	principal, _ := PrincipalFromContext(r.Context())
	roles, err := h.service.ListRoles(r.Context(), principal.TenantID)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	if roles == nil {
		roles = []Role{}
	}
	httpx.JSON(w, http.StatusOK, roles)
}
