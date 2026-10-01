#!/usr/bin/env bash
# Reproduce the RoboTool bugs listed in ../robotool-bugs.md.
#
# Usage: ROBOTOOL_HOME=/path/to/RoboTool [FDR_REFINES=/path/to/refines] ./reproduce.sh [B1 B2 ...]
#
# Each B<n> directory holds a one-file RoboChart project (model.rct) and, where
# the bug shows in FDR, a check (check.csp). The script copies the project to a
# temporary directory, runs RoboTool's headless CSP generator on it exactly as
# testsuite-runner does, and then runs FDR on the check.
set -u
here=$(cd "$(dirname "$0")" && pwd)
: "${ROBOTOOL_HOME:?set ROBOTOOL_HOME to the RoboTool installation}"
refines=${FDR_REFINES:-refines}
bugs=("$@")
[ ${#bugs[@]} -eq 0 ] && bugs=($(cd "$here" && ls -d B*/ | tr -d /))
for bug in "${bugs[@]}"; do
  work=$(mktemp -d)
  cp "$here/$bug/model.rct" "$work/"
  cat > "$work/.project" <<'XML'
<?xml version="1.0" encoding="UTF-8"?>
<projectDescription><name>RoboToolBug</name><comment></comment><projects/>
  <buildSpec><buildCommand><name>org.eclipse.xtext.ui.shared.xtextBuilder</name><arguments/></buildCommand></buildSpec>
  <natures><nature>org.eclipse.sirius.nature.modelingproject</nature><nature>org.eclipse.xtext.ui.shared.xtextNature</nature></natures>
</projectDescription>
XML
  echo "== $bug ($work)"
  java --add-opens java.base/java.lang=ALL-UNNAMED -Djava.awt.headless=true \
    -cp "$ROBOTOOL_HOME/plugins/*" circus.robocalc.robochart.generator.csp.Main true false "$work" \
    > "$work/robotool.log" 2>&1
  echo "RoboTool exit status: $?"
  grep -E 'ERROR|Case not treated|Generator failed with exception' "$work/robotool.log" | sort -u | head -5
  if [ -d "$work/csp-gen" ]; then
    for check in "$here/$bug/"*.csp; do
      [ -f "$check" ] || continue
      name=$(basename "$check")
      cp "$check" "$work/csp-gen/"
      echo "FDR check: $name"
      (cd "$work/csp-gen" && "$refines" --quiet --format plain "$name" 2>&1 \
        | grep -vE '^\s+(Found|Visited|Constructed|Reduced|Log:)|Welcome|License|^\s*$' | head -20)
    done
  fi
done
