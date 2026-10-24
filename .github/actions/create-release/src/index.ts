import { run } from "#run";
import type { ReleaseInputs, ReleaseOutputs } from "#types";
import { Toolkit } from "actions-toolkit";

Toolkit.run<ReleaseInputs, ReleaseOutputs>(run);
