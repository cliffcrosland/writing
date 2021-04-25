all:
	wasm-pack build --target web --out-name wasm --out-dir ./static

clean:
	(cd ./static && ls | grep -v index.html | xargs rm)
