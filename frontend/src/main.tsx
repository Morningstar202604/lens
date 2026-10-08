import { render } from 'preact';
import { App, ErrorBoundary } from './App';
import './index.css';

const root = document.getElementById('app');
if (root) {
  render(
    <ErrorBoundary>
      <App />
    </ErrorBoundary>,
    root
  );
}
