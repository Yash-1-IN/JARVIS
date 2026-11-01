#!/bin/bash
# Reference copy only. Paste this into your cloud environment's "Setup script"
# field on claude.ai/code (each developer sets up their own environment).
# See docs/spec.md §18.1. Confirm the package list against the current Tauri
# Linux prerequisites.
set -e
apt-get update
apt-get install -y libwebkit2gtk-4.1-dev build-essential libxdo-dev libssl-dev \
  libayatana-appindicator3-dev librsvg2-dev libasound2-dev pkg-config
