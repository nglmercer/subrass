import index from './demo/index.html';
const server = Bun.serve({
  port: 3001,
  async fetch(req) {
    const url = new URL(req.url);
    let path = url.pathname;
    const demoPath = "./demo/";
    const processPath = process.cwd();
    console.log(path);
    //pkg/subrass_bg.wasm.
    const allowed_ext = [
      ".js",
      ".mjs",
      ".html",
      ".wasm",
      ".css",
      ".ass"
    ];
    const htmlfile = Bun.file(index.index);
    //console.log(wasmfile)
    if (path === "/") return new Response(htmlfile);
    if (!allowed_ext.some((ext) => path.endsWith(ext))) {
      return new Response("Not Found", { status: 404 });
    }



    // For the main application logic
    const assets = Bun.file(`${demoPath}${path}`)
    const exists = await assets.exists();
    if (exists) return new Response(assets);
    else {
      console.log(`${processPath}${path}`);
      const assets = Bun.file(`${processPath}${path}`)

      const exists = await assets.exists();
      if (exists) return new Response(assets);
    }


    return new Response("Not Found", { status: 404 });
  }
});

console.log(`Server running at http://localhost:${server.port}`);
