#!/bin/bash

# Start/Stop the local DynamoDB docker container.
#
# Uses port 8000 on localhost.
#
# Usage: ./docker_dynamodb.sh <start/stop>
#
# The script pulls the "amazon/dynamodb-local" Docker image if we haven't already.

if [ -z $1 ]; then
  echo "Usage error: Expected argument \"start\" or \"stop\"."
  exit 1
elif [ "$1" == "start" ]; then
  hits=`docker ps -a | grep writing_dynamodb`
  if [ -z "$hits" ]; then
    echo "Pulling image: amazon/dynamodb-local, Starting container: writing_dynamodb"
    set -x
    docker run -d --name writing_dynamodb -p 127.0.0.1:8000:8000 amazon/dynamodb-local
  else
    echo "Starting container: writing_dynamodb"
    set -x
    docker start writing_dynamodb
  fi
elif [ "$1" == "stop" ]; then
  echo "Stopping container: writing_dynamodb"
  set -x
  docker stop writing_dynamodb
else
  echo "Usage error: Expected argument \"start\" or \"stop\"."
  exit 1
fi

