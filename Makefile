clean:
	docker compose down
	rm -rf ./pkg

build:
	wasm-pack build --release

up: build
	docker compose up