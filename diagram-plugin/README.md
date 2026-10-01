# Generate native RoboChart diagrams

This small RoboTool plug-in creates missing Sirius `SpecificationID` diagrams in a testsuite project's existing `representations.aird`. It reads model paths and diagram names from `case-index.csv`, creates one representation for each case, and saves the Sirius session. Re-running it leaves existing diagrams in place.

## Build

Use a JDK 17 and the RoboTool installation that will run the plug-in:

```sh
./build.sh /path/to/RoboTool /path/to/jdk-17/bin
```

The result is `build/org.robocalc.testsuitegenerator.diagrams.jar`. The bundle has no additional third-party dependencies.

## Run

Copy the built bundle into the RoboTool installation's `dropins/` directory, then launch the normal RoboTool workbench with the generated project path as a JVM property:

```sh
cp build/org.robocalc.testsuitegenerator.diagrams.jar /path/to/RoboTool/dropins/
/path/to/RoboTool/eclipse \
  -clean -nosplash -data /path/to/a/review-workspace \
  -vmargs -Dtestsuitegenerator.project=/absolute/path/to/testsuite-project
```

The startup hook imports the project into that one workspace if needed. Use a workspace that is not open in another Eclipse process. The plug-in creates diagrams after the workbench starts because Sirius diagram layout requires workbench services. RoboChart's `SpecificationID` description supplies the native content. The hook then runs native Arrange All commands on each diagram and its compartments, followed by bounded spacing adjustments for states, modules, and edge labels.

If RoboTool asks which representations to open, answer that dialog: while it is open it holds the workspace and the plug-in's final save waits. On success, stdout reports the number created and already present. The RoboTool window stays open for review. To prevent the startup hook from running on future RoboTool launches, omit `-Dtestsuitegenerator.project=...`; the generated diagrams remain in the project. Errors are printed to stderr.

## Refresh changed models

When a model already has a saved diagram, add `-Dtestsuitegenerator.refresh=all` after
`-vmargs` to refresh and arrange existing diagrams as well as creating missing
ones. To refresh selected diagrams, supply their comma-separated diagram names
from `case-index.csv`, for example
`-Dtestsuitegenerator.refresh=statement_bounded_scan_three`. This updates saved labels and
connections after regenerating a model. Omit the property for the usual pass
that adds missing diagrams.
