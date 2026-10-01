package org.robocalc.testsuitegenerator.diagrams;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Collection;
import java.util.List;

import org.eclipse.core.resources.IFile;
import org.eclipse.core.resources.IProject;
import org.eclipse.core.resources.IProjectDescription;
import org.eclipse.core.resources.IWorkspace;
import org.eclipse.core.resources.IWorkspaceDescription;
import org.eclipse.core.resources.ResourcesPlugin;
import org.eclipse.core.runtime.CoreException;
import org.eclipse.core.runtime.IProgressMonitor;
import org.eclipse.core.runtime.NullProgressMonitor;
import org.eclipse.emf.common.util.URI;
import org.eclipse.emf.ecore.EObject;
import org.eclipse.emf.ecore.resource.Resource;
import org.eclipse.emf.transaction.RecordingCommand;
import org.eclipse.gef.EditPart;
import org.eclipse.gef.commands.Command;
import org.eclipse.gef.commands.CompoundCommand;
import org.eclipse.gmf.runtime.diagram.ui.actions.ActionIds;
import org.eclipse.gmf.runtime.diagram.ui.editparts.DiagramEditPart;
import org.eclipse.gmf.runtime.diagram.ui.editparts.ShapeCompartmentEditPart;
import org.eclipse.gmf.runtime.diagram.ui.parts.DiagramCommandStack;
import org.eclipse.gmf.runtime.diagram.ui.parts.IDiagramWorkbenchPart;
import org.eclipse.gmf.runtime.diagram.ui.requests.ArrangeRequest;
import org.eclipse.gmf.runtime.notation.Bounds;
import org.eclipse.gmf.runtime.notation.Diagram;
import org.eclipse.gmf.runtime.notation.Edge;
import org.eclipse.gmf.runtime.notation.Node;
import org.eclipse.gmf.runtime.notation.NotationFactory;
import org.eclipse.gmf.runtime.notation.View;
import org.eclipse.sirius.business.api.dialect.DialectManager;
import org.eclipse.sirius.business.api.session.Session;
import org.eclipse.sirius.business.api.session.SessionManager;
import org.eclipse.sirius.diagram.DDiagram;
import org.eclipse.sirius.diagram.DDiagramElement;
import org.eclipse.sirius.ui.business.api.dialect.DialectUIManager;
import org.eclipse.sirius.viewpoint.DRepresentationDescriptor;
import org.eclipse.sirius.viewpoint.DRepresentation;
import org.eclipse.sirius.viewpoint.description.RepresentationDescription;
import org.eclipse.sirius.viewpoint.description.Viewpoint;
import org.eclipse.swt.widgets.Display;
import org.eclipse.ui.IEditorPart;
import org.eclipse.ui.PlatformUI;

/** Creates missing RoboChart diagrams and arranges the suite's diagrams in its .aird session. */
public final class GenerateDiagramsApplication {
    private static final String VIEWPOINT_NAME = "RoboChartID";
    private static final String DIAGRAM_DESCRIPTION_NAME = "SpecificationID";
    private static final String INDEX_NAME = "case-index.csv";
    private final IProgressMonitor monitor = new NullProgressMonitor();

    boolean generate(Path projectDirectory) {
        Session session = null;
        try {
            IProject project = openProject(projectDirectory);
            IFile airdFile = project.getFile("representations.aird");
            if (!airdFile.exists()) {
                throw new IllegalStateException("No representations.aird in " + projectDirectory);
            }

            URI airdUri = URI.createPlatformResourceURI(airdFile.getFullPath().toString(), true);
            session = SessionManager.INSTANCE.getSession(airdUri, monitor);
            if (session == null) {
                throw new IllegalStateException("Sirius did not create a session for " + airdUri);
            }
            if (!session.isOpen()) {
                System.out.println("Opening Sirius session: " + airdUri);
                session.open(monitor);
            }
            System.out.println("Sirius session opened");

            Viewpoint viewpoint = selectedViewpoint(session);
            RepresentationDescription diagramDescription = diagramDescription(viewpoint);
            List<CaseDiagram> cases = readCaseIndex(projectDirectory);
            int created = 0;
            int existing = 0;
            List<ExpectedDiagram> expected = new ArrayList<>();
            List<DRepresentation> toArrange = new ArrayList<>();

            for (CaseDiagram testCase : cases) {
                System.out.println("Creating/checking diagram: " + testCase.id());
                IFile modelFile = project.getFile(testCase.modelPath());
                if (!modelFile.exists()) {
                    throw new IllegalStateException("Missing model for " + testCase.id() + ": " + modelFile);
                }
                Resource modelResource = semanticResource(session, project, modelFile);
                if (modelResource == null) {
                    addSemanticResource(session,
                            URI.createPlatformResourceURI(modelFile.getFullPath().toString(), true));
                    modelResource = semanticResource(session, project, modelFile);
                }
                if (modelResource == null || modelResource.getContents().isEmpty()) {
                    throw new IllegalStateException("Could not load a RoboChart package from " + modelFile);
                }

                EObject root = modelResource.getContents().get(0);
                if (!"RCPackage".equals(root.eClass().getName())) {
                    throw new IllegalStateException("Expected RCPackage at the root of " + modelFile
                            + ", found " + root.eClass().getName());
                }
                ExpectedDiagram expectedDiagram = new ExpectedDiagram(testCase, root);
                expected.add(expectedDiagram);

                if (hasRepresentation(session, root, diagramDescription)) {
                    existing++;
                    String refresh = System.getProperty("testsuitegenerator.refresh", "");
                    if (refresh.equals("all") || java.util.Arrays.asList(refresh.split(","))
                            .contains(testCase.diagramName())) {
                        DRepresentation representation = representationFor(session, root, diagramDescription);
                        refreshRepresentation(session, representation);
                        toArrange.add(representation);
                        System.out.println("Refreshed diagram: " + testCase.id());
                    }
                    continue;
                }
                if (!DialectManager.INSTANCE.canCreate(root, diagramDescription)) {
                    throw new IllegalStateException("RoboChart cannot create a Specification diagram for "
                            + testCase.id());
                }
                toArrange.add(createVerifiedRepresentation(session, expectedDiagram, diagramDescription));
                System.out.println("Created diagram: " + testCase.id());
                created++;
            }

            // A representation can be dropped during a background post-build refresh. Repair any
            // omissions before arranging, rather than logging a successful create optimistically.
            for (ExpectedDiagram missing : missingRepresentations(session, expected, diagramDescription)) {
                toArrange.add(createVerifiedRepresentation(session, missing, diagramDescription));
                created++;
                System.out.println("Recovered missing diagram before arrange: " + missing.testCase().id());
            }

            int arranged = arrangeDiagrams(session, toArrange);
            System.out.println("Saving Sirius session");
            session.save(monitor);

            List<ExpectedDiagram> missingAfterSave = missingRepresentations(session, expected, diagramDescription);
            if (!missingAfterSave.isEmpty()) {
                List<DRepresentation> recovered = new ArrayList<>();
                for (ExpectedDiagram missing : missingAfterSave) {
                    recovered.add(createVerifiedRepresentation(session, missing, diagramDescription));
                    created++;
                    System.out.println("Recovered missing diagram after save: " + missing.testCase().id());
                }
                arranged += arrangeDiagrams(session, recovered);
                session.save(monitor);
                List<ExpectedDiagram> stillMissing = missingRepresentations(session, expected, diagramDescription);
                if (!stillMissing.isEmpty()) {
                    throw new IllegalStateException("Sirius did not retain " + stillMissing.size()
                            + " of " + expected.size() + " expected diagrams after a retry: "
                            + stillMissing.stream().map(item -> item.testCase().id()).toList());
                }
            }
            System.out.println("RoboChart case diagrams: created " + created + ", already present " + existing
                    + ", auto-arranged " + arranged + ", total " + cases.size() + " in " + airdFile.getFullPath());
            return true;
        } catch (Exception exception) {
            exception.printStackTrace(System.err);
            return false;
        } finally {
            if (session != null && session.isOpen()) {
                session.close(monitor);
            }
        }
    }

    private IProject openProject(Path projectDirectory) throws CoreException {
        if (!Files.isDirectory(projectDirectory)) {
            throw new IllegalArgumentException("Project directory does not exist: " + projectDirectory);
        }
        String projectName = projectDirectory.getFileName().toString();
        IWorkspace workspace = ResourcesPlugin.getWorkspace();
        IWorkspaceDescription workspaceDescription = workspace.getDescription();
        if (workspaceDescription.isAutoBuilding()) {
            workspaceDescription.setAutoBuilding(false);
            workspace.setDescription(workspaceDescription);
        }
        IProject project = workspace.getRoot().getProject(projectName);
        if (!project.exists()) {
            IProjectDescription description = workspace.newProjectDescription(projectName);
            description.setLocation(new org.eclipse.core.runtime.Path(projectDirectory.toString()));
            project.create(description, monitor);
        } else if (project.getLocation() == null
                || !project.getLocation().toFile().toPath().toAbsolutePath().normalize().equals(projectDirectory)) {
            throw new IllegalStateException("Workspace project " + projectName + " points to " + project.getLocation()
                    + ", expected " + projectDirectory);
        }
        if (!project.isOpen()) {
            project.open(monitor);
        }
        return project;
    }

    private static Viewpoint selectedViewpoint(Session session) {
        return session.getSelectedViewpoints(false).stream()
                .filter(viewpoint -> VIEWPOINT_NAME.equals(viewpoint.getName()))
                .findFirst()
                .orElseThrow(() -> new IllegalStateException("The session has no selected RoboChart viewpoint"));
    }

    private static RepresentationDescription diagramDescription(Viewpoint viewpoint) {
        return viewpoint.getOwnedRepresentations().stream()
                .filter(description -> DIAGRAM_DESCRIPTION_NAME.equals(description.getName()))
                .findFirst()
                .orElseThrow(() -> new IllegalStateException("RoboChart viewpoint lacks SpecificationID"));
    }

    private static List<CaseDiagram> readCaseIndex(Path projectDirectory) throws Exception {
        Path index = projectDirectory.resolve(INDEX_NAME);
        if (!Files.isRegularFile(index)) {
            throw new IllegalStateException("Missing " + index);
        }
        List<String> lines = Files.readAllLines(index, StandardCharsets.UTF_8);
        if (lines.isEmpty() || !("kind,feature,id,model,diagram,expected_status".equals(lines.get(0).trim())
                || "kind,feature,id,model,diagram,expected_status,timed".equals(lines.get(0).trim()))) {
            throw new IllegalStateException("Unexpected case-index.csv header");
        }
        List<CaseDiagram> cases = new ArrayList<>();
        for (int i = 1; i < lines.size(); i++) {
            if (lines.get(i).isBlank()) {
                continue;
            }
            String[] columns = lines.get(i).split(",", -1);
            if (columns.length != lines.get(0).split(",", -1).length) {
                throw new IllegalStateException("Malformed case-index.csv line " + (i + 1));
            }
            Path model = Path.of(columns[3]).normalize();
            if (model.isAbsolute() || model.startsWith("..") || !model.toString().endsWith(".rct")) {
                throw new IllegalStateException("Invalid model path in case-index.csv line " + (i + 1));
            }
            cases.add(new CaseDiagram(columns[2], model.toString().replace('\\', '/'), columns[4]));
        }
        if (cases.isEmpty()) {
            throw new IllegalStateException("case-index.csv contains no models");
        }
        return cases;
    }

    private static Resource semanticResource(Session session, IProject project, IFile modelFile) {
        String platformUri = URI.createPlatformResourceURI(modelFile.getFullPath().toString(), true).toString();
        String fileUri = URI.createFileURI(modelFile.getLocation().toFile().getAbsolutePath()).toString();
        String relativeSuffix = "/" + project.getName() + "/" + modelFile.getProjectRelativePath().toString()
                .replace('\\', '/');
        for (Resource resource : session.getSemanticResources()) {
            String uri = resource.getURI().toString().replace('\\', '/');
            if (uri.equals(platformUri) || uri.equals(fileUri) || uri.endsWith(relativeSuffix)) {
                return resource;
            }
        }
        return null;
    }

    /** Adds a newly encountered model through the session's write transaction. */
    private void addSemanticResource(Session session, URI resourceUri) {
        var domain = session.getTransactionalEditingDomain();
        domain.getCommandStack().execute(new RecordingCommand(domain) {
            @Override
            protected void doExecute() {
                session.addSemanticResource(resourceUri, monitor);
            }
        });
    }

    private static boolean hasRepresentation(Session session, EObject root,
            RepresentationDescription diagramDescription) {
        Collection<DRepresentationDescriptor> descriptors =
                DialectManager.INSTANCE.getRepresentationDescriptors(root, session);
        return descriptors.stream().anyMatch(descriptor -> descriptor.getTarget() == root
                && descriptor.getDescription() == diagramDescription);
    }

    private void refreshRepresentation(Session session, DRepresentation representation) {
        session.getTransactionalEditingDomain().getCommandStack().execute(
                new RecordingCommand(session.getTransactionalEditingDomain()) {
                    @Override
                    protected void doExecute() {
                        DialectManager.INSTANCE.refresh(representation, monitor);
                    }
                });
    }

    private DRepresentation createVerifiedRepresentation(Session session, ExpectedDiagram expected,
            RepresentationDescription diagramDescription) {
        for (int attempt = 1; attempt <= 2; attempt++) {
            var domain = session.getTransactionalEditingDomain();
            DRepresentation[] created = new DRepresentation[1];
            domain.getCommandStack().execute(new RecordingCommand(domain) {
                @Override
                protected void doExecute() {
                    created[0] = DialectManager.INSTANCE.createRepresentation(
                            expected.testCase().diagramName(), expected.root(), diagramDescription,
                            session, monitor);
                }
            });
            if (hasRepresentation(session, expected.root(), diagramDescription)) {
                if (created[0] != null) {
                    return created[0];
                }
                return representationFor(session, expected.root(), diagramDescription);
            }
            System.out.println("Diagram registration check failed for " + expected.testCase().id()
                    + " (attempt " + attempt + "); " + (attempt == 1 ? "retrying" : "giving up"));
        }
        throw new IllegalStateException("Sirius failed to register diagram " + expected.testCase().id());
    }

    private static DRepresentation representationFor(Session session, EObject root,
            RepresentationDescription diagramDescription) {
        return DialectManager.INSTANCE.getRepresentationDescriptors(root, session).stream()
                .filter(descriptor -> descriptor.getTarget() == root
                        && descriptor.getDescription() == diagramDescription)
                .map(DRepresentationDescriptor::getRepresentation)
                .filter(representation -> representation != null)
                .findFirst()
                .orElseThrow(() -> new IllegalStateException("Representation descriptor has no representation"));
    }

    private static List<ExpectedDiagram> missingRepresentations(Session session,
            List<ExpectedDiagram> expected, RepresentationDescription diagramDescription) {
        return expected.stream()
                .filter(item -> !hasRepresentation(session, item.root(), diagramDescription))
                .toList();
    }

    /** Opens every suite diagram and runs Sirius/GMF's native Arrange All command. */
    private int arrangeDiagrams(Session session, RepresentationDescription diagramDescription) throws Exception {
        Collection<DRepresentation> representations =
                DialectManager.INSTANCE.getRepresentations(diagramDescription, session);
        return arrangeDiagrams(session, representations);
    }

    private int arrangeDiagrams(Session session, Collection<DRepresentation> representations) throws Exception {
        Display display = PlatformUI.getWorkbench().getDisplay();
        int arranged = 0;
        for (DRepresentation representation : representations) {
            if (!(representation instanceof DDiagram)) {
                continue;
            }
            final Exception[] failure = new Exception[1];
            final boolean[] didArrange = new boolean[1];
            Runnable arrange = () -> {
                try {
                    arrangeDiagram(session, representation);
                    didArrange[0] = true;
                } catch (Exception exception) {
                    failure[0] = exception;
                }
            };
            if (display.getThread() == Thread.currentThread()) {
                arrange.run();
            } else {
                display.syncExec(arrange);
            }
            if (failure[0] != null) {
                throw failure[0];
            }
            if (didArrange[0]) {
                arranged++;
            }
        }
        return arranged;
    }

    private void arrangeDiagram(Session session, DRepresentation representation) throws Exception {
        IEditorPart editor = DialectUIManager.INSTANCE.openEditor(session, representation, monitor);
        if (!(editor instanceof IDiagramWorkbenchPart diagramEditor)) {
            if (editor != null) {
                DialectUIManager.INSTANCE.closeEditor(editor, true);
            }
            throw new IllegalStateException("Sirius did not open a GMF diagram editor for "
                    + representation.getName());
        }

        try {
            DiagramEditPart diagramEditPart = diagramEditor.getDiagramEditPart();
            if (diagramEditPart == null) {
                throw new IllegalStateException("Sirius diagram editor has no diagram edit part for "
                        + representation.getName());
            }
            DiagramCommandStack commandStack = diagramEditor.getDiagramEditDomain().getDiagramCommandStack();
            int layouts = arrangeNestedContainers(diagramEditPart, commandStack);
            int notationChanges = improveNestedNotationLayout(session, diagramEditPart);
            System.out.println("Auto-arranged " + representation.getName() + " (" + layouts
                    + " diagram/container layout commands, " + notationChanges
                    + " nested spacing adjustments)");
        } finally {
            if (!DialectUIManager.INSTANCE.closeEditor(editor, false)) {
                throw new IllegalStateException("Could not close Sirius editor for " + representation.getName());
            }
        }
    }

    /** Arranges a container's descendants first, then lays out its direct children. */
    private int arrangeNestedContainers(EditPart part, DiagramCommandStack commandStack) {
        int layouts = 0;
        for (Object child : new ArrayList<>(part.getChildren())) {
            if (child instanceof EditPart childEditPart) {
                layouts += arrangeNestedContainers(childEditPart, commandStack);
            }
        }

        if (part instanceof DiagramEditPart) {
            layouts += arrangeChildren(part, commandStack);
        } else if (part instanceof ShapeCompartmentEditPart) {
            layouts += arrangeCompartment(part, commandStack);
        }
        return layouts;
    }

    /** Sends Arrange All to the compartment that owns nested shapes, as GMF does for a nested selection. */
    private int arrangeCompartment(EditPart compartment, DiagramCommandStack commandStack) {
        ArrangeRequest request = new ArrangeRequest(ActionIds.ACTION_ARRANGE_ALL);
        request.setPartsToArrange(List.of(compartment));
        Command command = compartment.getCommand(request);
        if (command == null || !command.canExecute()) {
            return 0;
        }
        commandStack.execute(command, monitor);
        return 1;
    }

    private int arrangeChildren(EditPart parent, DiagramCommandStack commandStack) {
        List<EditPart> children = new ArrayList<>();
        for (Object child : parent.getChildren()) {
            if (child instanceof EditPart editPart) {
                children.add(editPart);
            }
        }
        if (children.isEmpty()) {
            return 0;
        }

        ArrangeRequest request = new ArrangeRequest(ActionIds.ACTION_ARRANGE_ALL);
        request.setPartsToArrange(children);
        CompoundCommand commands = new CompoundCommand("Arrange Sirius diagram contents");
        for (EditPart child : children) {
            Command command = child.getCommand(request);
            if (command != null && command.canExecute()) {
                commands.add(command);
            }
        }
        if (commands.isEmpty() || !commands.canExecute()) {
            return 0;
        }
        commandStack.execute(commands, monitor);
        return 1;
    }

    /**
     * The RoboChart VSM uses fixed 24px state symbols and leaves their region bounds implicit.
     * Arrange All consequently packs the symbols tightly. Give state regions explicit space,
     * spread their direct children, and fan out the three native edge-label anchors.
     */
    private int improveNestedNotationLayout(Session session, DiagramEditPart diagramEditPart) {
        if (!(diagramEditPart.getModel() instanceof Diagram diagram)) {
            return 0;
        }
        int[] changes = new int[1];
        var domain = session.getTransactionalEditingDomain();
        domain.getCommandStack().execute(new RecordingCommand(domain) {
            @Override
            protected void doExecute() {
                changes[0] = improveNotationView(diagram);
                for (Object edge : diagram.getEdges()) {
                    if (edge instanceof Edge notationEdge) {
                        changes[0] += fanOutEdgeLabels(notationEdge);
                    }
                }
            }
        });
        return changes[0];
    }

    private int improveNotationView(View view) {
        int changes = 0;
        if (view instanceof Node node && isStateNodeRegion(node)) {
            changes += layoutStateNodeRegion(node);
        }
        if (view instanceof Node node && isModuleNodesCompartment(node)) {
            changes += moveModuleSiblings(node);
        }
        for (Object child : new ArrayList<>(view.getChildren())) {
            if (child instanceof View childView) {
                changes += improveNotationView(childView);
            }
        }
        return changes;
    }

    private boolean isStateNodeRegion(Node node) {
        if (!("3008".equals(node.getType()) && node.getElement() instanceof DDiagramElement element)
                || (element.getName() != null && !element.getName().isBlank())) {
            return false;
        }
        String targetType = semanticType(node);
        return "StateMachineDef".equals(targetType) || "State".equals(targetType);
    }

    private int layoutStateNodeRegion(Node region) {
        String ownerType = semanticType(region);
        int cursorX = 40;
        int largestChildHeight = 0;
        int childCount = 0;
        int changes = 0;
        List<Node> stateNodes = new ArrayList<>();
        collectStateNodes(region, stateNodes);
        if (stateNodes.isEmpty()) {
            System.out.println("State region " + ownerType + " notation children: "
                    + region.getChildren().stream()
                            .map(child -> child instanceof View view ? view.getType() : child.getClass().getName())
                            .toList());
        }
        for (Node state : stateNodes) {
            boolean nestedRegion = hasStateNodeRegion(state);
            int width = "3008".equals(state.getType()) ? (nestedRegion ? 400 : 220) : 24;
            int height = "3008".equals(state.getType()) ? (nestedRegion ? 240 : 140) : 24;
            changes += setBounds(state, cursorX, 60, width, height);
            cursorX += width + 70;
            largestChildHeight = Math.max(largestChildHeight, height);
            childCount++;
        }
        if (childCount > 0) {
            int minimumWidth = "StateMachineDef".equals(ownerType) ? 360 : 320;
            int minimumHeight = "StateMachineDef".equals(ownerType) ? 200 : 150;
            changes += setBounds(region, null, null, Math.max(minimumWidth, cursorX + 20),
                    Math.max(minimumHeight, largestChildHeight + 100));
        }
        System.out.println("State region " + ownerType + " contains " + childCount
                + " state node(s); updated " + changes + " bounds");
        return changes;
    }

    /** State nodes are nested one level below the region in its Sirius compartment view. */
    private void collectStateNodes(Node compartment, List<Node> stateNodes) {
        for (Object child : new ArrayList<>(compartment.getChildren())) {
            if (!(child instanceof Node childNode)) {
                continue;
            }
            if (childNode.getElement() instanceof DDiagramElement
                    && ("3007".equals(childNode.getType()) || "3008".equals(childNode.getType()))) {
                stateNodes.add(childNode);
            } else {
                collectStateNodes(childNode, stateNodes);
            }
        }
    }

    private boolean hasStateNodeRegion(View view) {
        for (Object child : view.getChildren()) {
            if (child instanceof Node node && isStateNodeRegion(node)) {
                return true;
            }
            if (child instanceof View childView && hasStateNodeRegion(childView)) {
                return true;
            }
        }
        return false;
    }

    private boolean isModuleNodesCompartment(Node node) {
        return "7002".equals(node.getType()) && node.eContainer() instanceof Node owner
                && "RCModule".equals(semanticType(owner));
    }

    /** Keeps the module's platform/reference nodes below the enlarged controller contents. */
    private int moveModuleSiblings(Node moduleCompartment) {
        int changes = 0;
        int referenceY = 440;
        for (Object child : new ArrayList<>(moduleCompartment.getChildren())) {
            if (!(child instanceof Node node) || node.getElement() == null) {
                continue;
            }
            if ("ControllerDef".equals(semanticType(node))) {
                changes += setPosition(node, 30, 32);
            } else {
                changes += setPosition(node, 30, referenceY);
                referenceY += 150;
            }
        }
        return changes;
    }

    private int fanOutEdgeLabels(Edge edge) {
        int changes = 0;
        for (Object child : edge.getChildren()) {
            if (!(child instanceof Node label)) {
                continue;
            }
            Integer y = switch (label.getType()) {
                case "6001" -> -20;
                case "6002" -> 0;
                case "6003" -> 20;
                default -> null;
            };
            if (y != null) {
                changes += setPosition(label, null, y);
            }
        }
        return changes;
    }

    private String semanticType(Node node) {
        if (node.getElement() instanceof DDiagramElement element && element.getTarget() != null) {
            return element.getTarget().eClass().getName();
        }
        return "";
    }

    private int setPosition(Node node, Integer x, Integer y) {
        return setBounds(node, x, y, null, null);
    }

    private int setBounds(Node node, Integer x, Integer y, Integer width, Integer height) {
        Bounds bounds;
        if (node.getLayoutConstraint() instanceof Bounds existing) {
            bounds = existing;
        } else {
            bounds = NotationFactory.eINSTANCE.createBounds();
            node.setLayoutConstraint(bounds);
        }
        boolean changed = false;
        if (x != null && bounds.getX() != x) {
            bounds.setX(x);
            changed = true;
        }
        if (y != null && bounds.getY() != y) {
            bounds.setY(y);
            changed = true;
        }
        if (width != null && bounds.getWidth() != width) {
            bounds.setWidth(width);
            changed = true;
        }
        if (height != null && bounds.getHeight() != height) {
            bounds.setHeight(height);
            changed = true;
        }
        return changed ? 1 : 0;
    }

    private record CaseDiagram(String id, String modelPath, String diagramName) { }
    private record ExpectedDiagram(CaseDiagram testCase, EObject root) { }
}
