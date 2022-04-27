.PHONY: help build start
.DEFAULT_GOAL := help

help:
	@fgrep -h "###" $(MAKEFILE_LIST) | fgrep -v fgrep | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-30s\033[0m %s\n", $$1, $$2}'

build: ### Build the docker image
	docker build -t rio-fastly-nginx-gzip .

start: ### Start the server
	docker run --name rio-fastly-nginx-gzip --rm -it -p 9999:80 -d -v `pwd`:/usr/share/nginx/html:ro rio-fastly-nginx-gzip

stop: ### Stop the server
	docker stop rio-fastly-nginx-gzip
