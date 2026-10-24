#!/bin/sh
# Publish the clickable prototype to https://layr.ws (S3 bucket layr-ws-prototype behind CloudFront ETP5AYPASWZMY).
# Needs the AWS profile of account 134250228509. Usage: AWS_PROFILE=ifdotpy mockup/tools/publish.sh
set -eu
cd "$(dirname "$0")/.."
python3 tools/build.py
python3 tools/check.py
aws s3 sync . s3://layr-ws-prototype/ --exclude "tools/*" --exclude "README.md" --exclude ".*" --delete --cache-control "public, max-age=300" --only-show-errors
aws cloudfront create-invalidation --distribution-id ETP5AYPASWZMY --paths "/*" --query Invalidation.Id --output text
