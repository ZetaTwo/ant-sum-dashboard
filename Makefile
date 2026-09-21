.PHONY: install build build-backend build-backend-windows build-frontend test test-backend test-frontend \
        dev dev-backend dev-hardware dev-frontend run clean

# Install frontend dependencies (run once, or after pulling package.json changes)
install:
	cd frontend && pnpm install

# Build everything for production (release binary + static frontend assets)
build: build-frontend build-backend

build-backend:
	cd backend && cargo build --release

# Cross-compile the backend for Windows (requires the x86_64-pc-windows-gnu
# rustup target and a mingw-w64 toolchain; libusb is vendored and compiled
# via mingw, no separate Windows libusb install needed)
build-backend-windows:
	cd backend && cargo build --release --target x86_64-pc-windows-gnu

build-frontend:
	cd frontend && pnpm run build

# Run backend + frontend unit tests / typechecks
test: test-backend test-frontend

test-backend:
	cd backend && cargo test

test-frontend:
	cd frontend && pnpm exec tsc --noEmit

# Run backend (--simulate) and the Vite dev server together; Ctrl-C stops both
dev:
	@trap 'kill 0' INT TERM EXIT; \
	(cd backend && cargo run -- --simulate --port 8080) & \
	(cd frontend && pnpm run dev) & \
	wait

# Backend only, synthetic data, no frontend
dev-backend:
	cd backend && cargo run -- --simulate --port 8080

# Backend only, real ANT+ USB hardware, no frontend
dev-hardware:
	cd backend && cargo run -- --port 8080

dev-frontend:
	cd frontend && pnpm run dev

# Run the production build (requires `make build` first)
run:
	./backend/target/release/ant-sum-dashboard --port 8080 --static-dir ../frontend/dist

clean:
	cd backend && cargo clean
	rm -rf frontend/dist
