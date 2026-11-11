#!/bin/bash
# Double-click in Finder: builds PureMap.app and opens it.
cd "$(dirname "$0")"
bash packaging/macos/build-app.sh 2>&1 | tee build.log
echo ""
echo "构建结束，正在打开 PureMap.app ……"
open PureMap.app || true
