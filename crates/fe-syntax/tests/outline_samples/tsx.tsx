import React from "react";

interface Props {
  title: string;
  onClick(): void;
}

type State = { count: number };

enum Mode {
  Light,
  Dark,
}

class Counter extends React.Component<Props, State> {
  render() {
    return <button onClick={this.props.onClick}>{this.props.title}</button>;
  }
}

export const App = ({ title }: Props) => {
  return <Counter title={title} onClick={() => {}} />;
};

export function Header(): JSX.Element {
  return <h1>Header</h1>;
}
