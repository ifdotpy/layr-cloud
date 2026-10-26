import http from 'node:http';import https from 'node:https';import fs from 'node:fs';import path from 'node:path';
// Host adapter for the disposable VM only. Forward 19013 to layr-sa:19003 with SSH.
// Run: node tests/site-accept-web.mjs web/dist
// The fixture uses a self-signed test certificate; this adapter serves only on loopback.
const root=path.resolve(process.argv[2]);
http.createServer((req,res)=>{
 if(req.url.startsWith('/api/')||req.url.startsWith('/v1/')) {
  const headers={...req.headers,host:'localhost'};delete headers.origin;
  const up=https.request({hostname:'127.0.0.1',port:19013,path:req.url,method:req.method,headers,rejectUnauthorized:false},reply=>{res.writeHead(reply.statusCode,reply.headers);reply.pipe(res)});up.on('error',e=>{res.writeHead(502);res.end(e.message)});req.pipe(up);return;
 }
 const file=path.join(root,new URL(req.url,'http://localhost').pathname==='/'?'index.html':new URL(req.url,'http://localhost').pathname);
 if(!file.startsWith(root)||!fs.existsSync(file)){res.writeHead(404);res.end();return}
 const ext=path.extname(file);res.setHeader('content-type',({'.html':'text/html','.js':'text/javascript','.css':'text/css','.wasm':'application/wasm'})[ext]||'application/octet-stream');fs.createReadStream(file).pipe(res);
}).listen(4200,'127.0.0.1',()=>console.log('READY http://localhost:4200'));
