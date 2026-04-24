import * as React from 'react';

interface Props {
  name: string;
  count?: number;
}

export const Greeter: React.FC<Props> = ({ name, count = 1 }) => {
  return (
    <div className="greeter">
      {/* JSX comment */}
      <h1>Hello, {name}!</h1>
      {Array.from({ length: count }, (_, i) => (
        <p key={i}>Repeat {i + 1}</p>
      ))}
    </div>
  );
};
