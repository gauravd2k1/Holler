//! Offline login. Verifies against the Argon2id hash cached in `app_user`
//! (edge/database::auth) — never depends on network availability, and never
//! returns or logs credential material (ADR-011).

use tauri::State;

use crate::dto::AuthenticatedPrincipal;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

pub fn login_impl(
    state: &AppState,
    email: &str,
    password: &str,
) -> AppResult<AuthenticatedPrincipal> {
    let db = state.db.lock().map_err(|_| AppError {
        code: "LOCK_POISONED",
        message: "database lock poisoned".into(),
    })?;

    let user = holler_edge_database::repo::verify_offline_login(
        db.connection(),
        &state.outlet_id,
        email,
        password,
    )?;

    AuthenticatedPrincipal::from_app_user(&user).map_err(|e| AppError {
        code: "MALFORMED_PERMISSIONS",
        message: format!("stored permissions_json is invalid: {e}"),
    })
}

#[tauri::command]
pub fn login(
    state: State<'_, AppState>,
    email: String,
    password: String,
) -> AppResult<AuthenticatedPrincipal> {
    login_impl(&state, &email, &password)
}
