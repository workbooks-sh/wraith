const API = {
  motionNet: { adEngine },
};

import * as adEngine from './ad-engine';

function createUseQuery<T>(_def: T): unknown {
  return null;
}

export const useMetaAssetsTeamQuery = createUseQuery(
  API.motionNet.adEngine.getMetaAssetsTeam
);
