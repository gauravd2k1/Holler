package auth

import (
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

func (h *Handlers) login(w http.ResponseWriter, r *http.Request) {
	var req loginRequest
	if err := httpx.DecodeJSON(r, &req); err != nil {
		httpx.Error(w, err)
		return
	}
	tenantID := r.Header.Get("X-Tenant-ID")
	if tenantID == "" || req.Email == "" || req.Password == "" || req.OutletID == "" {
		httpx.Error(w, httpx.ErrUnauthorized)
		return
	}

	result, err := h.service.Login(r.Context(), tenantID, req.Email, req.Password, req.OutletID)
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
		h.service.Logout(req.RefreshToken)
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
	httpx.JSON(w, http.StatusOK, toUserResponses(users))
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
	httpx.JSON(w, http.StatusCreated, toUserResponse(user))
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
	httpx.JSON(w, http.StatusOK, toUserResponse(user))
}

func (h *Handlers) listRoles(w http.ResponseWriter, r *http.Request) {
	principal, _ := PrincipalFromContext(r.Context())
	roles, err := h.service.ListRoles(r.Context(), principal.TenantID)
	if err != nil {
		httpx.Error(w, err)
		return
	}
	httpx.JSON(w, http.StatusOK, toRoleResponses(roles))
}

// --- wire shapes (mirror contracts.AppUser / contracts.Role; never carry a
// hash field) ---

type roleAssignmentWire struct {
	ID       string  `json:"id"`
	RoleID   string  `json:"role_id"`
	RoleCode string  `json:"role_code"`
	OutletID *string `json:"outlet_id"`
}

type userWire struct {
	ID       string               `json:"id"`
	TenantID string               `json:"tenant_id"`
	Email    string               `json:"email"`
	FullName string               `json:"full_name"`
	IsActive bool                 `json:"is_active"`
	Roles    []roleAssignmentWire `json:"roles"`
}

func toUserResponse(u User) userWire {
	roles := make([]roleAssignmentWire, 0, len(u.Roles))
	for _, a := range u.Roles {
		roles = append(roles, roleAssignmentWire{ID: a.ID, RoleID: a.RoleID, RoleCode: string(a.RoleCode), OutletID: a.OutletID})
	}
	return userWire{
		ID:       u.ID,
		TenantID: u.TenantID,
		Email:    u.Email,
		FullName: u.FullName,
		IsActive: u.IsActive,
		Roles:    roles,
	}
}

func toUserResponses(users []User) []userWire {
	out := make([]userWire, 0, len(users))
	for _, u := range users {
		out = append(out, toUserResponse(u))
	}
	return out
}

type roleWire struct {
	ID          string   `json:"id"`
	TenantID    string   `json:"tenant_id"`
	Code        string   `json:"code"`
	Name        string   `json:"name"`
	Permissions []string `json:"permissions"`
}

func toRoleResponses(roles []Role) []roleWire {
	out := make([]roleWire, 0, len(roles))
	for _, r := range roles {
		perms := make([]string, 0, len(r.Permissions))
		for _, p := range r.Permissions {
			perms = append(perms, string(p))
		}
		out = append(out, roleWire{ID: r.ID, TenantID: r.TenantID, Code: string(r.Code), Name: r.Name, Permissions: perms})
	}
	return out
}
