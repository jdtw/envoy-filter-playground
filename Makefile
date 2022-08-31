clean:
	docker compose down
	rm -rf ./pkg

build:
	wasm-pack build --release --out-dir '../pkg' 'filter'
	wasm-pack build --release --out-dir '../pkg' 'service'

up: build
	docker compose up
