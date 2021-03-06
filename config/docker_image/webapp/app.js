const http = require('http');
const os = require('os');

console.log('Kubia server starting ...');

var handler = function(req, resp) {
  console.log("Received request from " + req.connection.remoteAddress);
  resp.writeHead(200);
  resp.end("You've hit the server " + os.hostname + "\n");
};

var www = http.createServer(handler);
www.listen(8080);
