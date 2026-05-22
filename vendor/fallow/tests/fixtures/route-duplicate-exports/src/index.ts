// Entry point
import './routes/foo/router';
import './routes/bar/router';
import { formatDate } from './shared/utils';
import { formatDate as formatDate2 } from './shared/helpers';

console.log(formatDate(), formatDate2());
