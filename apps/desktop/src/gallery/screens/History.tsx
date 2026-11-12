import { Button } from "../../components/Button";
import { Page } from "../../components/Page";
import { Row, Section } from "../../components/Rows";
import { Tag } from "../../components/Tag";
import { history } from "../sample";

// Everything Kiln changed, newest first, each with what it touched and a way back.
export function History() {
  return (
    <Page title="History">
      <p className="-mt-1 mb-6 max-w-[64ch] text-[13px] text-text-2">
        Every change Kiln makes to this PC is recorded here, from the app or from the command line, and can be undone. Undo restores files byte for byte.
      </p>
      {history.map((d) => (
        <Section key={d.day} title={d.day}>
          {d.items.map((h) => (
            <Row
              key={h.time + h.what}
              columns="64px minmax(240px,1fr) auto"
              actions={
                h.undone ? undefined : (
                  <Button variant="quiet" size="sm">
                    Undo
                  </Button>
                )
              }
            >
              <span className="figures text-xs text-text-3">{h.time}</span>
              <span className={h.undone ? "text-text-2" : ""}>
                {h.what}
                {h.undone && (
                  <span className="ml-2">
                    <Tag>undone</Tag>
                  </span>
                )}
                <span className="block text-xs text-text-3">Changed: {h.detail}</span>
              </span>
            </Row>
          ))}
        </Section>
      ))}
    </Page>
  );
}
