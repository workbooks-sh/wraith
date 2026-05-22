import { sharedUtil } from 'shared';
import { formatDate } from '@workspace/utils';
import { deepHelper } from '@workspace/utils/src/deep';
export const main = () => sharedUtil() + formatDate(new Date()) + deepHelper();
