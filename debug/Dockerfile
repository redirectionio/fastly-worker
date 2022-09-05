#syntax=docker/dockerfile:1.4

FROM nginx:1.13.6

COPY <<EOF /etc/nginx/nginx.conf
user  nginx;
worker_processes  auto;

error_log  /var/log/nginx/error.log notice;
pid        /var/run/nginx.pid;

events {
    worker_connections  1024;
}

http {
    include       /etc/nginx/mime.types;
    default_type  application/octet-stream;

    charset UTF-8;

    sendfile        on;

    gzip on;
    keepalive_timeout  65;

    server {
        listen       80;
        server_name  localhost;

        location / {
            root   /usr/share/nginx/html;
            index  index.html index.htm;
        }

        # allow POST on static pages
        error_page  405 =200 \$uri;
    }
}
EOF
