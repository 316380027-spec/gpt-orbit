import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import App from './App';

describe('App', () => {
  afterEach(cleanup);

  it('renders the product name while the backend starts', () => {
    render(<App />);
    expect(screen.getByRole('main', { name: 'Gpt Orbit' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Gpt Orbit' })).toBeInTheDocument();
    expect(screen.queryByRole('group', { name: 'Gpt Orbit Weekly' })).not.toBeInTheDocument();
  });

  it('renders the weekly-only surface for the weekly variant', () => {
    render(<App variant="weekly" />);

    expect(screen.getByRole('group', { name: 'Gpt Orbit Weekly' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Gpt Orbit Weekly' })).not.toBeInTheDocument();
    expect(screen.getByLabelText('额度重置次数暂不可用')).toBeInTheDocument();
    expect(screen.queryByText(/5H/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/5 小时/)).not.toBeInTheDocument();
  });
});
