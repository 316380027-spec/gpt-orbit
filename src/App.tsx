import { APP_VARIANT, type AppVariant } from './appVariant';
import StandardApp from './app/StandardApp';
import WeeklyApp from './app/WeeklyApp';

interface AppProps {
  variant?: AppVariant;
}

export default function App({ variant = APP_VARIANT }: AppProps) {
  return variant === 'weekly' ? <WeeklyApp /> : <StandardApp />;
}
