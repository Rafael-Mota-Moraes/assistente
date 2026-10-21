import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { FileTree } from "./EditorPanel";

function renderTree(files: string[]) {
  const pastas = new Set<string>();
  for (const f of files) {
    const partes = f.split("/");
    for (let i = 1; i < partes.length; i++) pastas.add(partes.slice(0, i).join("/"));
  }
  render(
    <FileTree
      files={files}
      status={[]}
      selected={null}
      expanded={pastas}
      onToggle={() => {}}
      onSelect={() => {}}
    />,
  );
}

const recuo = (botao: HTMLElement) => Number.parseInt(botao.style.paddingLeft, 10);

describe("FileTree", () => {
  it("arquivo dentro da pasta fica à direita do nome da pasta", () => {
    renderTree(["src/nested/filho.ts", "src/pais.ts"]);

    const pasta = screen.getByRole("button", { name: "src" });
    const arquivo = screen.getByRole("button", { name: "filho.ts" });

    // O arquivo entra um nível mais fundo que a pasta que o contém.
    expect(recuo(arquivo)).toBeGreaterThan(recuo(pasta));

    // E reserva o espaço do chevron da pasta: sem isso o nome do arquivo ficaria
    // alinhado com o da pasta, e não à direita dele.
    const espaco = arquivo.querySelector('span[aria-hidden="true"]');
    expect(espaco).not.toBeNull();
    expect(espaco).toHaveClass("size-3.5");
  });

  it("cada nível aninha mais para a direita", () => {
    renderTree(["a/b/c/fundo.ts"]);

    const raiz = screen.getByRole("button", { name: "a" });
    const meio = screen.getByRole("button", { name: "b" });
    const fundo = screen.getByRole("button", { name: "fundo.ts" });

    expect(recuo(meio)).toBeGreaterThan(recuo(raiz));
    expect(recuo(fundo)).toBeGreaterThan(recuo(meio));
  });
});
