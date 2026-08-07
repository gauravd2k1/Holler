// Command api is the Holler Cloud backend entrypoint.
//
// Milestone 0: health endpoint only. No business logic — see CLAUDE.md
// EXCLUDES for Milestone 0.
package main

import (
	"log"
	"net/http"
	"os"

	"github.com/holler/backend/internal/health"
)

func main() {
	port := os.Getenv("PORT")
	if port == "" {
		port = "8080"
	}

	mux := http.NewServeMux()
	mux.HandleFunc("/health", health.Handler)

	log.Printf("holler backend listening on :%s", port)
	if err := http.ListenAndServe(":"+port, mux); err != nil {
		log.Fatalf("server failed: %v", err)
	}
}
