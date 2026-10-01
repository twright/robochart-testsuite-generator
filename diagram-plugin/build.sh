#!/bin/sh
set -eu

if [ "$#" -lt 2 ] || [ "$#" -gt 3 ]; then
    printf 'Usage: %s ROBOTOOL_INSTALL JDK_BIN [OUTPUT_DIR]\n' "$0" >&2
    exit 2
fi

TASK_PLUGIN_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
TASK_ROBOTOOL_INSTALL=$1
TASK_JDK_BIN=$2
TASK_OUTPUT_DIR=${3:-"$TASK_PLUGIN_DIR/build"}
mkdir -p "$TASK_OUTPUT_DIR/classes"

"$TASK_JDK_BIN/javac" --release 17 \
    -cp "$TASK_ROBOTOOL_INSTALL/plugins/*" \
    -d "$TASK_OUTPUT_DIR/classes" \
    "$TASK_PLUGIN_DIR/src/org/robocalc/testsuitegenerator/diagrams/GenerateDiagramsApplication.java" \
    "$TASK_PLUGIN_DIR/src/org/robocalc/testsuitegenerator/diagrams/GenerateDiagramsStartup.java"

"$TASK_JDK_BIN/jar" cfm "$TASK_OUTPUT_DIR/org.robocalc.testsuitegenerator.diagrams.jar" \
    "$TASK_PLUGIN_DIR/META-INF/MANIFEST.MF" \
    -C "$TASK_OUTPUT_DIR/classes" . \
    -C "$TASK_PLUGIN_DIR" plugin.xml

printf 'Built %s\n' "$TASK_OUTPUT_DIR/org.robocalc.testsuitegenerator.diagrams.jar"
