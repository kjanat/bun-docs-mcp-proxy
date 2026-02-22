import { Toolkit } from "actions-toolkit";
import { run } from "./run";
import type { ReleaseInputs, ReleaseOutputs } from "./types";

Toolkit.run<ReleaseInputs, ReleaseOutputs>(run);
