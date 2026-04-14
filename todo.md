In cursor and other apps clicking anywhere on a tree branch in the tree widget will open or close that branch. Right now we only open and close on the actual triangle icon. Let's expand it to the whole row

The inspector should discover the layout in realtime and build the tree. The should be a complete tree always.

If I close the 3D Demo window it is still showing in the Widget Inpector. It should have closed the widget and the inspect should be getting the real scene so it should go away from the inspector. The inspector should be discovering what is the actual widget hierarchy. Also, when I close the 3D Demo window the scene should not be re-drawing every frame. It should only draw on demand. The 3D Demo was causing a draw every frame to it would animate but the entire scene should only re-draw when any widget requests a re-draw.

It seems like the widget inspector is hard coded to this scene. Is that true? It needs to discover what is on screen. The goal is this is a general debugging tool we can include in any widget project to understand what is going on with our layouts and test and fix them (like F12 in js).