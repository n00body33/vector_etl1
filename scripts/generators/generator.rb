class Generator
  attr_reader :guides

  def initialize(guides)
    @guides = guides
  end

  def generate
    raise MethodMissinError.new
  end

  private
    def alternatives(alternatives)
      if alternatives.any?
        alternative_links = alternatives.collect do |alternative|
          "* [#{component_name(alternative)}][#{component_short_link(alternative)}]"
        end

        content = <<~EOF

        ### Alternatives

        Finally, consider the following alternatives:

        #{alternative_links.join("\n")}
        EOF

        content.strip
      else
        ""
      end
    end

    def beta(component)
      if component.beta?
        content = <<~EOF
        {% hint style="warning" %}
        The #{component_name(component)} is in beta. Please see the current [enhancements](#{component_issues_link(component, enhancement_label)}) and [bugs](#{component_issues_link(component, bug_label)}) for known issues. We kindly ask that you [add any missing issues](#{new_component_issue_url(component)}) as it will help shape the roadmap of this component.
        {% endhint %}
        EOF
        content.strip
      else
        ""
      end
    end

    def bug_label
      "Type: Bug"
    end

    def component_label(component)
      "#{component_type(component).humanize}: #{component.name}"
    end

    def component_issues_link(component, *labels)
      label_url(component_label(component), *labels)
    end

    def component_name(component)
      "`#{component.name}` #{component_type(component)}"
    end

    def component_short_link(component)
      "#{component.name}_#{component_type(component)}"
    end

    def component_source_url(component)
      "#{REPO_SRC_ROOT}/#{component_type(component)}/#{component.name}.rs"
    end

    def component_type(component)
      if component.is_a?(Sink)
        "sink"
      elsif component.is_a?(Source)
        "source"
      elsif component.is_a?(Transform)
        "transform"
      else
        raise("Unknown component: #{component.inspect}")
      end
    end

    def enhancement_label
      "Type: Enhancement"
    end

    def event_type_links(types)
      types.collect do |type|
        "[`#{type}`][#{type}_event]"
      end
    end

    def guides_section(component)
      guide_links =
        guides.
          select { |guide| guide.send(component.type.pluralize).include?(component.name) }.
          collect do |guide|
            "* [**#{guide.title} Guide**](#{guide.file_path})"
          end

      content =
        if guide_links.any?
          <<~EOF
          ## Guides

          #{guide_links.join("\n")}
          EOF
        else
          ""
        end

      content.lstrip.strip
    end

    def label_url(*labels)
      label_queries = labels.collect { |label| "label:\"#{label}\"" }
      query = "is:open is:issue #{label_queries.join(" ")}"
      REPO_ISSUES_ROOT + '?' + {"q" => query}.to_query
    end

    def new_component_issue_url(component, *labels)
      new_issue_url(component_label(component), *labels)
    end

    def new_issue_url(*labels)
      REPO_ISSUES_ROOT + '/new?' + {"labels" => labels.join(",")}.to_query
    end

    def outputs_section(component, prefix = nil)
      if component.outputs.length > 1
        tabs =
          component.outputs.collect do |output|
            content =
              <<~EOF
              {% tab title="#{output.type}" %}
              #{output.body}
              {% endtab %}
              EOF

            content.strip
          end

        content = 
          <<~EOF
          ## I/O

          #{prefix}
          
          {% tabs %}
          #{tabs.join("\n")}
          {% endtabs %}
          EOF

        content.strip
      elsif component.outputs.length > 0
        content = 
          <<~EOF
          ## I/O

          #{prefix}

          #{component.outputs.fetch(0).body}
          EOF

        content.strip
      else
        content = 
          <<~EOF
          ## I/O

          #{prefix}
          EOF

        content.strip
      end
    end

    def remove_markdown_links(regex)
      regex.
        gsub(/\[([^\]]+)\]\(([^)]+)\)/, '\1').
        gsub(/\[([^\]]+)\]\[([^)]+)\]/, '\1')
    end

    def resource_links(resources)
      links = resources.collect do |resource|
        "* [**#{resource.name}**](#{resource.url})"
      end
      links.join("\n")
    end

    def resources(component)
      content = <<~EOF
      ## Resources

      * [**Issues**](#{component_issues_link(component)}) - [enhancements](#{component_issues_link(component, enhancement_label)}) - [bugs](#{component_issues_link(component, bug_label)})
      * [**Source code**](#{component_source_url(component)})
      #{resource_links(component.resources)}
      EOF
      content.strip
    end

    def tags(component)
      tags = []

      tags << "`status: #{component.beta? ? "beta" : "stable"}`"

      if component.respond_to?("input_types") && component.input_types.any?
        tags << "`input: #{component.input_types.to_sentence}`"
      end

      if component.respond_to?("output_types") && component.output_types.any?
        tags << "`output: #{component.output_types.to_sentence}`"
      end

      if component.respond_to?("delivery_guarantee")
        tags << "`guarantee: [#{component.delivery_guarantee}][#{component.delivery_guarantee}_delivery]`"
      end

      tags.join(" ")
    end

    def troubleshooting(component)
      content = <<~EOF
      ## Troubleshooting

      The best place to start with troubleshooting is to check the
      [Vector logs][monitoring_logs]. This is typically located at
      `/var/log/vector.log`, then proceed to follow the
      [Troubleshooting Guide][troubleshooting].

      If the [Troubleshooting Guide][troubleshooting] does not resolve your
      issue, please:

      1. Check for any [open #{component_type(component)} issues](#{component_issues_link(component)}).
      2. [Search the forum][search_forum] for any similar issues.
      2. Reach out to the [community][community] for help.
      
      #{alternatives(component.alternatives)}
      EOF
      content.strip
    end

    def warning
      <<~EOF
      <!---
      !!!WARNING!!!!

      This file is autogenerated! Please do not manually edit this file.
      Instead, please modify the contents of `dist/config/schema.toml`.
      -->
      EOF
    end
end