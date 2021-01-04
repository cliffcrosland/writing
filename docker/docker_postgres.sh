#!/bin/bash

# Start/Stop the postgres docker container.
#
# Uses port 5432 on localhost.
#
# Usage: ./docker_postgres.sh <start/stop>
#
# The script pulls the "postgres:13" Docker image if we haven't already.

if [ -z $1 ]; then
  echo "Usage error: Expected argument \"start\" or \"stop\"."
  exit 1
elif [ "$1" == "start" ]; then
  if [ -z "$WRITING_PG_DEV_PASSWORD" ]; then
    echo "Usage error: The WRITING_PG_DEV_PASSWORD env variable must be set to a value."
    exit 1
  fi
  hits=`docker ps -a | grep writing_postgres`
  if [ -z "$hits" ]; then
    echo "Pulling image: postgres:13, Starting container: postgres"
    set -x
    docker run -d --name writing_postgres -p 127.0.0.1:5432:5432 -e POSTGRES_PASSWORD=$WRITING_PG_DEV_PASSWORD postgres:13
  else
    echo "Starting container: writing_postgres"
    set -x
    docker start writing_postgres
  fi
elif [ "$1" == "stop" ]; then
  echo "Stopping container: writing_postgres"
  set -x
  docker stop postgres
else
  echo "Usage error: Expected argument \"start\" or \"stop\"."
  exit 1
fi

