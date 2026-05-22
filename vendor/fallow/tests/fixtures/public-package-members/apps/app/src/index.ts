import {
  PublicStatus,
  WorkspaceService,
} from '../../../packages/public-lib/src/index';

const service = new WorkspaceService();

export const main = () => service.usedMethod() + PublicStatus.Active;
