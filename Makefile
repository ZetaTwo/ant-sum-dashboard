.PHONY: install build build-backend build-frontend test test-backend test-frontend \
        dev dev-backend dev-hardware dev-frontend run clean

# Install frontend dependencies (run once, or after pulling package.json changes)
install:
	cd frontend && npm install

# Build everything for production (release binary + static frontend assets)
build: build-frontend build-backend

build-backend:
	cd backend && cargo build --release

build-frontend:
	cd frontend && npm run build

# Run backend + frontend unit tests / typechecks
test: test-backend test-frontend

test-backend:
	cd backend && cargo test

test-frontend:
	cd frontend && npx tsc --noEmit

# Run backend (--simulate) and the Vite dev server together; Ctrl-C stops both
dev:
	@trap 'kill 0' INT TERM EXIT; \
	(cd backend && cargo run -- --simulate --port 8080) & \
	(cd frontend && npm run dev) & \
	wait

# Backend only, synthetic data, no frontend
dev-backend:
	cd backend && cargo run -- --simulate --port 8080

# Backend only, real ANT+ USB hardware, no frontend
dev-hardware:
	cd backend && cargo run -- --port 8080

dev-frontend:
	cd frontend && npm run dev

# Run the production build (requires `make build` first)
run:
	./backend/target/release/ant-sum-dashboard --port 8080 --static-dir ../frontend/dist

clean:
	cd backend && cargo clean
	rm -rf frontend/dist
