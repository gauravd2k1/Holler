.PHONY: dev down test test-backend lint fmt

dev:
	docker compose up --build

down:
	docker compose down

test: test-backend

test-backend:
	cd backend && go test ./...

lint:
	cd backend && go vet ./...

fmt:
	cd backend && gofmt -l .
