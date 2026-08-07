// Package health provides the backend's liveness endpoint.
package health

import (
	"encoding/json"
	"net/http"
	"time"
)

type response struct {
	Status string    `json:"status"`
	Time   time.Time `json:"time"`
}

// Handler responds 200 OK with a minimal liveness payload. It intentionally
// does not check downstream dependencies (Postgres/Redis/NATS) yet — that
// arrives with the modules that actually depend on them.
func Handler(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(response{
		Status: "ok",
		Time:   time.Now().UTC(),
	})
}
