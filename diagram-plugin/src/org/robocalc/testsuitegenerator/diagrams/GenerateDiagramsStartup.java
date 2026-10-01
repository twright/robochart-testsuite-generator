package org.robocalc.testsuitegenerator.diagrams;

import java.nio.file.Path;
import org.eclipse.ui.IStartup;

/** Runs one automatic diagram pass after RoboTool's graphical workbench opens. */
public final class GenerateDiagramsStartup implements IStartup {
    @Override
    public void earlyStartup() {
        String project = System.getProperty("testsuitegenerator.project");
        if (project == null || project.isBlank()) {
            return;
        }
        if (!new GenerateDiagramsApplication().generate(Path.of(project))) {
            System.err.println("RoboChart case diagram generation failed for " + project);
        }
    }
}
